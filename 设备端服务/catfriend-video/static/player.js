(() => {
  "use strict";

  const byId = (id) => document.getElementById(id);
  const image = byId("stream");
  const backoff = [1000, 2000, 5000, 10000];
  const stateNames = { starting: "启动中", streaming: "正在出帧", reconnecting: "源重连中", stopped: "已停止" };
  let active = false;
  let pageAway = false;
  let generation = 0;
  let requestId = 0;
  let controller = null;
  let requestTimer = null;
  let pollTimer = null;
  let clockTimer = null;
  let retryTimer = null;
  let retryAt = 0;
  let retryStep = 0;
  let streamRunning = false;
  let streamStartedAt = 0;
  let streamResponded = false;
  let streamError = false;
  let sourceBlocked = false;
  let statusFailed = false;
  let status = null;
  let receivedAt = 0;
  let lastSequence = null;
  let progressAt = 0;
  let streamToken = 0;
  let manualRetryAt = 0;

  const text = (id, value) => { byId(id).textContent = value; };
  const finite = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
  const available = () => !pageAway && !document.hidden && navigator.onLine;

  function frameAge(now) {
    return status && status.frame_age_ms !== null ? status.frame_age_ms + now - receivedAt : null;
  }

  function sourceReady(now = performance.now()) {
    if (!status || statusFailed || !status.healthy || status.state !== "streaming") return false;
    const fps = status.measured_fps > 0 ? status.measured_fps : status.configured_fps;
    const limit = Math.max(5000, fps > 0 ? 3000 / fps : 5000);
    const age = frameAge(now);
    return !(age !== null && age >= limit) &&
      !(lastSequence !== null && now - progressAt >= limit);
  }

  function stopStream() {
    streamToken += 1;
    image.onload = null;
    image.onerror = null;
    image.removeAttribute("src");
    streamRunning = false;
    streamResponded = false;
  }

  function clearRetry() {
    clearTimeout(retryTimer);
    retryTimer = null;
    retryAt = 0;
  }

  function scheduleRetry() {
    if (!active || retryTimer !== null || !sourceReady()) return;
    const delay = backoff[Math.min(retryStep, backoff.length - 1)];
    retryStep += 1;
    retryAt = performance.now() + delay;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      retryAt = 0;
      if (active && sourceReady()) startStream();
      render();
    }, delay);
  }

  function startStream() {
    if (!active || !sourceReady() || streamRunning || retryTimer !== null) return;
    const token = ++streamToken;
    const session = generation;
    streamRunning = true;
    streamStartedAt = performance.now();
    streamResponded = false;
    streamError = false;
    sourceBlocked = false;
    // MJPEG load is not a per-frame event. Never use it as a frame clock.
    image.onload = () => {
      if (!active || token !== streamToken || session !== generation) return;
      streamResponded = true;
      render();
    };
    image.onerror = () => {
      if (!active || token !== streamToken || session !== generation) return;
      stopStream();
      streamError = true;
      scheduleRetry();
      render();
    };
    // One stream per attempt; status polling never replaces an active image.
    image.src = `/stream.mjpg?connection=${Date.now()}-${token}`;
  }

  function validateStatus(value) {
    if (!value || !["test_pattern", "rpicam"].includes(value.source) ||
        !Object.prototype.hasOwnProperty.call(stateNames, value.state) || typeof value.healthy !== "boolean" ||
        !["width", "height", "configured_fps", "measured_fps", "viewers"].every((key) => finite(value[key])) ||
        !(value.frame_sequence === null || finite(value.frame_sequence)) ||
        !(value.frame_age_ms === null || finite(value.frame_age_ms)) ||
        !(value.last_error === null || typeof value.last_error === "string")) {
      throw new Error("状态响应格式无效");
    }
    return value;
  }

  async function pollStatus() {
    if (!active || controller !== null) return;
    const session = generation;
    const id = ++requestId;
    const abort = new AbortController();
    controller = abort;
    requestTimer = setTimeout(() => abort.abort(), 1800);
    try {
      const response = await fetch("/api/status", { cache: "no-store", signal: abort.signal });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const next = validateStatus(await response.json());
      if (!active || session !== generation || id !== requestId || abort.signal.aborted) return;
      const now = performance.now();
      if (next.frame_sequence !== lastSequence || !status || next.source !== status.source) {
        progressAt = now;
      }
      lastSequence = next.frame_sequence;
      status = next;
      receivedAt = now;
      statusFailed = false;
      text("updated-at", `更新于 ${new Date().toLocaleTimeString("zh-CN", { hour12: false })}`);
      updateMetrics();
      reconcile();
    } catch (error) {
      if (!active || session !== generation || id !== requestId) return;
      statusFailed = true;
      reconcile();
    } finally {
      if (session === generation && id === requestId) {
        clearTimeout(requestTimer);
        requestTimer = null;
        controller = null;
        render();
      }
    }
  }

  function updateMetrics() {
    if (!status) return;
    byId("test-label").hidden = status.source !== "test_pattern";
    text("metric-source", status.source === "test_pattern" ? "测试画面" : "设备摄像头");
    text("metric-state", `${stateNames[status.state]}${status.healthy ? "" : " · 未就绪"}`);
    text("metric-resolution", `${status.width} × ${status.height}`);
    text("metric-fps", `${status.measured_fps.toFixed(1)} / ${status.configured_fps.toFixed(1)} fps`);
    text("metric-viewers", String(status.viewers));
    byId("error-notice").hidden = !status.last_error;
    text("last-error", status.last_error || "");
  }

  function reconcile() {
    if (!active) return;
    if (!sourceReady()) {
      if (streamRunning) stopStream();
      clearRetry();
      sourceBlocked = true;
    } else if (!streamRunning && retryTimer === null) {
      if (sourceBlocked || streamError) scheduleRetry();
      else startStream();
    }
    // Reset backoff only after a sustained healthy source, not on each poll.
    if (streamRunning && sourceReady() && performance.now() - streamStartedAt >= 15000) retryStep = 0;
    render();
  }

  function render() {
    const now = performance.now();
    let title = "正在加载";
    let detail = "正在获取视频源状态，请稍候。";
    let tone = "waiting";
    let pill = "正在连接";
    let overlay = true;
    let spinning = true;
    if (pageAway || document.hidden) {
      title = "观看已暂停"; detail = "返回页面后将自动恢复连接。"; pill = "已暂停"; spinning = false;
    } else if (!navigator.onLine) {
      title = "网络已离线"; detail = "检查网络连接，联网后将自动重连。"; pill = "网络离线"; tone = "bad"; spinning = false;
    } else if (statusFailed) {
      title = "设备暂时无法连接"; detail = "正在重新获取设备状态，也可点击重新连接。"; pill = "连接中断"; tone = "bad";
    } else if (status && !sourceReady(now)) {
      title = "画面暂停";
      detail = status.state === "starting" ? "视频源正在启动，准备好后自动恢复。" :
        status.state === "reconnecting" ? "视频源正在重连，恢复出帧后自动连接。" :
        status.state === "stopped" ? "视频源已停止，等待设备恢复。" :
        !status.healthy ? "视频源尚未就绪，正在等待恢复。" : "视频源暂未更新帧，正在等待恢复。";
      pill = "源画面暂停"; tone = "bad"; spinning = status.state === "starting" || status.state === "reconnecting";
    } else if (retryTimer !== null || streamError) {
      title = "正在重新连接画面";
      const seconds = Math.max(1, Math.ceil((retryAt - now) / 1000));
      detail = retryTimer !== null ? `${seconds} 秒后重试画面连接。` : "等待视频源恢复后重试。";
      pill = "画面重连中";
    } else if (streamRunning) {
      title = "视频源正在出帧"; detail = "画面连接已发起，可打开截图查看设备当前画面。";
      pill = "源正在出帧"; tone = "good"; overlay = false; spinning = false;
    }
    byId("connection-pill").dataset.tone = tone;
    text("connection-label", pill);
    text("status-title", title);
    text("status-detail", detail);
    text("view-title", title);
    text("view-detail", detail);
    byId("view-overlay").hidden = !overlay;
    byId("loader").hidden = !spinning;
    byId("viewport").setAttribute("aria-busy", String(spinning));
    text("stream-label", streamRunning ? (streamResponded ? "画面连接已响应" : "已请求实时画面") : "画面连接未开启");
    byId("retry-button").disabled = !active || now < manualRetryAt;
    if (status) {
      const age = frameAge(now);
      text("metric-age", age === null ? "—" : `${(age / 1000).toFixed(1)} 秒${statusFailed || !active ? " · 待更新" : ""}`);
    }
  }

  function suspend() {
    active = false;
    generation += 1;
    requestId += 1;
    clearInterval(pollTimer);
    clearInterval(clockTimer);
    clearTimeout(requestTimer);
    clearRetry();
    pollTimer = clockTimer = requestTimer = null;
    if (controller) controller.abort();
    controller = null;
    stopStream();
    render();
  }

  function resume() {
    if (!available()) { if (active) suspend(); else render(); return; }
    if (active) return;
    active = true;
    generation += 1;
    status = null;
    statusFailed = false;
    sourceBlocked = false;
    streamError = false;
    lastSequence = null;
    progressAt = performance.now();
    retryStep = 0;
    text("updated-at", "等待状态更新");
    // Prior-session metrics must not look current after a network/page return.
    for (const key of ["source", "state", "resolution", "fps", "age", "viewers"]) text(`metric-${key}`, "—");
    byId("test-label").hidden = true;
    byId("error-notice").hidden = true;
    pollTimer = setInterval(pollStatus, 2000);
    clockTimer = setInterval(reconcile, 1000);
    pollStatus();
    render();
  }

  byId("retry-button").addEventListener("click", () => {
    if (!active || performance.now() < manualRetryAt) return;
    manualRetryAt = performance.now() + 2000;
    suspend();
    resume();
  });
  window.addEventListener("offline", suspend);
  window.addEventListener("online", resume);
  document.addEventListener("visibilitychange", () => { if (document.hidden) suspend(); else resume(); });
  window.addEventListener("pagehide", () => { pageAway = true; suspend(); });
  window.addEventListener("pageshow", () => { pageAway = false; resume(); });
  resume();
})();