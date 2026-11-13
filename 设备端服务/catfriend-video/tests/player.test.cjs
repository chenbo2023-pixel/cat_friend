"use strict";

// Run from catfriend-video: node --test tests/player.test.cjs
// No browser, network, wall-clock sleeps, packages, or production-code hooks.
const test = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");

const playerPath = path.join(__dirname, "../static/player.js");
const playerCode = fs.readFileSync(playerPath, "utf8");
const ids = [
  "stream", "updated-at", "test-label", "metric-source", "metric-state",
  "metric-resolution", "metric-fps", "metric-viewers", "metric-age",
  "error-notice", "last-error", "connection-pill", "connection-label",
  "status-title", "status-detail", "view-title", "view-detail",
  "view-overlay", "loader", "viewport", "stream-label", "retry-button",
];

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// Drain fetch -> json -> reconcile -> finally continuations after each timer.
async function flush() {
  for (let i = 0; i < 12; i += 1) await Promise.resolve();
}

function healthy(overrides = {}) {
  return {
    source: "test_pattern", state: "streaming", healthy: true,
    width: 640, height: 480, configured_fps: 10, measured_fps: 9.5,
    viewers: 1, frame_sequence: 1, frame_age_ms: 0, last_error: null,
    ...overrides,
  };
}

function harness({ ignoreAbort = false, hidden = false } = {}) {
  let now = 0;
  let timerId = 0;
  let autoStatus = null;
  const timers = new Map();
  const requests = [];
  const connections = [];
  const disconnections = [];
  const htmlWrites = [];

  function eventTarget() {
    const listeners = new Map();
    return {
      addEventListener(type, listener) {
        if (!listeners.has(type)) listeners.set(type, []);
        listeners.get(type).push(listener);
      },
      dispatch(type) {
        for (const listener of listeners.get(type) || []) listener({ type });
      },
    };
  }

  const elements = new Map(ids.map((id) => {
    const attributes = new Map();
    const element = {
      ...eventTarget(), textContent: "", hidden: false, disabled: false,
      dataset: {}, onload: null, onerror: null,
      setAttribute(name, value) {
        if (name === "src") this.src = value;
        else attributes.set(name, String(value));
      },
      getAttribute(name) { return attributes.get(name) ?? null; },
      removeAttribute(name) {
        if (name === "src" && attributes.has(name)) {
          disconnections.push({ at: now, src: attributes.get(name) });
        }
        attributes.delete(name);
      },
    };
    Object.defineProperties(element, {
      src: {
        get() { return attributes.get("src") || ""; },
        set(value) {
          attributes.set("src", String(value));
          connections.push({ at: now, src: String(value) });
        },
      },
      innerHTML: {
        get() { return ""; },
        set(value) { htmlWrites.push({ id, value }); },
      },
    });
    return [id, element];
  }));
  const document = {
    ...eventTarget(), hidden,
    getElementById(id) {
      assert.ok(elements.has(id), `DOM mock missing #${id}`);
      return elements.get(id);
    },
  };
  const window = eventTarget();
  const navigator = { onLine: true };

  function schedule(callback, delay, interval = false) {
    const id = ++timerId;
    timers.set(id, { callback, at: now + delay, interval, delay });
    return id;
  }

  function fetch(url, options) {
    assert.equal(url, "/api/status");
    assert.equal(options.cache, "no-store");
    const pending = deferred();
    const request = {
      at: now, signal: options.signal, settled: false,
      respond(value, { ok = true, status = 200, body } = {}) {
        request.settled = true;
        pending.resolve({ ok, status, json: () => body || Promise.resolve(value) });
      },
    };
    requests.push(request);
    options.signal.addEventListener("abort", () => {
      if (!ignoreAbort && !request.settled) {
        request.settled = true;
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        pending.reject(error);
      }
    }, { once: true });
    if (autoStatus) request.respond(autoStatus(now, requests.length));
    return pending.promise;
  }

  class FakeDate extends Date {
    constructor(...args) {
      super(...(args.length ? args : [1700000000000 + now]));
    }
    static now() { return 1700000000000 + now; }
  }

  const context = vm.createContext({
    document, window, navigator, fetch, AbortController, Date: FakeDate,
    performance: { now: () => now },
    setTimeout: (fn, delay) => schedule(fn, delay),
    clearTimeout: (id) => timers.delete(id),
    setInterval: (fn, delay) => schedule(fn, delay, true),
    clearInterval: (id) => timers.delete(id),
  });
  vm.runInContext(playerCode, context, { filename: playerPath });

  return {
    document, window, navigator, requests, connections, disconnections,
    htmlWrites, timers,
    element: (id) => elements.get(id),
    image: elements.get("stream"),
    get now() { return now; },
    autoStatus(fn) { autoStatus = fn; },
    async reply(index, value, options) {
      requests[index].respond(value, options);
      await flush();
    },
    async advance(milliseconds) {
      const target = now + milliseconds;
      let callbacks = 0;
      while (true) {
        const due = [...timers.entries()]
          .filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
        if (!due) break;
        assert.ok(++callbacks < 10000, "virtual timer runaway");
        const [id, timer] = due;
        now = timer.at;
        if (timer.interval) timer.at += timer.delay;
        else timers.delete(id);
        timer.callback();
        await flush();
      }
      now = target;
      await flush();
    },
    async visibility(value) {
      document.hidden = value;
      document.dispatch("visibilitychange");
      await flush();
    },
  };
}

async function streaming(options) {
  const h = harness(options);
  h.autoStatus((now) => healthy({ frame_sequence: 1 + Math.floor(now / 1000) }));
  await h.reply(0, healthy());
  assert.equal(h.connections.length, 1, "healthy source opens the initial MJPEG");
  return h;
}

test("normal status polling maintains one MJPEG connection; load is not a frame clock", async () => {
  const h = await streaming();
  const src = h.image.src;
  h.image.onload();
  await h.advance(20000);
  assert.equal(h.requests.length, 11);
  assert.equal(h.connections.length, 1);
  assert.equal(h.disconnections.length, 0);
  assert.equal(h.image.src, src);
  assert.equal(h.element("stream-label").textContent, "画面连接已响应");
  assert.equal(h.element("view-overlay").hidden, true);
});

test("img.onerror reconnects with capped 1s/2s/5s/10s backoff, unaffected by polling", async () => {
  const h = await streaming();
  const urls = new Set([h.image.src]);
  for (const delay of [1000, 2000, 5000, 10000, 10000]) {
    const previousConnections = h.connections.length;
    const failedAt = h.now;
    h.image.onerror();
    assert.equal(h.image.getAttribute("src"), null);
    assert.equal(h.element("view-overlay").hidden, false);
    await h.advance(delay - 1);
    assert.equal(h.connections.length, previousConnections, "must not reconnect early");
    await h.advance(1);
    assert.equal(h.connections.length, previousConnections + 1);
    assert.equal(h.connections.at(-1).at, failedAt + delay);
    urls.add(h.image.src);
  }
  assert.equal(urls.size, h.connections.length, "each retry uses a fresh URL");
});

test("15 seconds of sustained healthy streaming resets the retry backoff", async () => {
  const h = await streaming();
  h.image.onerror();
  await h.advance(1000);
  h.image.onerror();
  await h.advance(2000);
  await h.advance(15000);
  const before = h.connections.length;
  h.image.onerror();
  await h.advance(999);
  assert.equal(h.connections.length, before);
  await h.advance(1);
  assert.equal(h.connections.length, before + 1);
});

test("unchanged frame sequence pauses even with fresh frame_age_ms, then progress resumes", async () => {
  const h = await streaming();
  h.autoStatus(() => healthy({ frame_sequence: 1, frame_age_ms: 0 }));
  await h.advance(5000);
  assert.equal(h.image.getAttribute("src"), null);
  assert.equal(h.element("status-title").textContent, "画面暂停");
  assert.match(h.element("status-detail").textContent, /暂未更新帧/);
  await h.advance(2000);
  assert.equal(h.connections.length, 1, "stalled source must not churn streams");
  h.autoStatus(() => healthy({ frame_sequence: 2 }));
  await h.advance(1000); // Fresh status at t=8s schedules recovery at t=9s.
  assert.equal(h.connections.length, 1);
  await h.advance(999);
  assert.equal(h.connections.length, 1);
  await h.advance(1);
  assert.equal(h.connections.length, 2);
});

test("old frame age pauses despite increasing sequence and resumes after fresh frames", async () => {
  const h = await streaming();
  h.autoStatus((now) => healthy({ frame_sequence: now, frame_age_ms: 5000 }));
  await h.advance(2000);
  assert.equal(h.image.getAttribute("src"), null);
  assert.equal(h.element("status-title").textContent, "画面暂停");
  h.autoStatus((now) => healthy({ frame_sequence: now, frame_age_ms: 0 }));
  await h.advance(2000);
  assert.equal(h.connections.length, 1);
  await h.advance(1000);
  assert.equal(h.connections.length, 2);
});

test("unhealthy source closes MJPEG, cancels pending retry, and recovers only when healthy", async () => {
  const h = await streaming();
  h.image.onerror();
  await h.advance(1000);
  h.image.onerror(); // Second retry would fire at t=3s.
  h.autoStatus((now) => healthy({
    frame_sequence: now, healthy: false, state: "reconnecting",
  }));
  await h.advance(1000);
  assert.equal(h.image.getAttribute("src"), null);
  assert.match(h.element("status-detail").textContent, /视频源正在重连/);
  await h.advance(4000);
  assert.equal(h.connections.length, 2, "unhealthy status cancels scheduled retry");
  h.autoStatus((now) => healthy({ frame_sequence: now }));
  await h.advance(2000);
  assert.equal(h.connections.length, 2);
  await h.advance(5000);
  assert.equal(h.connections.length, 3);
});

test("status timeout aborts at 1800ms, pauses existing stream, and allows a subsequent poll", async () => {
  const h = await streaming();
  h.autoStatus(null);
  await h.advance(2000);
  const hung = h.requests[1];
  await h.advance(1799);
  assert.equal(hung.signal.aborted, false);
  assert.ok(h.image.src);
  await h.advance(1);
  assert.equal(hung.signal.aborted, true);
  assert.equal(h.image.getAttribute("src"), null);
  assert.equal(h.element("status-title").textContent, "设备暂时无法连接");
  h.autoStatus((now) => healthy({ frame_sequence: now }));
  await h.advance(200);
  assert.equal(h.requests.length, 3, "timed-out request does not block future polling");
  await h.advance(1000);
  assert.equal(h.connections.length, 2);
});

test("late response from an aborted session cannot overwrite or close the new session", async () => {
  const h = harness({ ignoreAbort: true });
  const oldRequest = h.requests[0];
  await h.visibility(true);
  assert.equal(oldRequest.signal.aborted, true);
  await h.visibility(false);
  assert.equal(h.requests.length, 2);
  await h.reply(1, healthy({ width: 800, frame_sequence: 20 }));
  const newSrc = h.image.src;
  await h.reply(0, healthy({
    width: 123, healthy: false, state: "stopped", last_error: "old session",
  }));
  assert.equal(h.element("metric-resolution").textContent, "800 × 480");
  assert.equal(h.element("last-error").textContent, "");
  assert.equal(h.image.src, newSrc);
  assert.equal(h.connections.length, 1);
  h.autoStatus((now) => healthy({ width: 800, frame_sequence: 21 + now }));
  await h.advance(2000);
  assert.equal(h.requests.length, 3, "old finally must not break the new session polling");
});

test("late JSON completion cannot clear the new session request timeout", async () => {
  const h = harness({ ignoreAbort: true });
  const body = deferred();
  await h.reply(0, null, { body: body.promise });
  await h.visibility(true);
  await h.visibility(false);
  assert.equal(h.requests.length, 2);
  body.resolve(healthy({ last_error: "stale JSON" }));
  await flush();
  assert.equal(h.element("metric-resolution").textContent, "—");
  assert.equal(h.connections.length, 0);
  await h.advance(1800);
  assert.equal(h.requests[1].signal.aborted, true, "new request timeout survives old finally");
  await h.reply(1, healthy({ width: 999 }));
  assert.equal(h.connections.length, 0, "response after timeout cannot open MJPEG");
  assert.equal(h.element("metric-resolution").textContent, "—");
});

test("hidden page closes the stream and timers; returning opens exactly one fresh connection", async () => {
  const h = await streaming();
  const oldOnError = h.image.onerror;
  const oldOnLoad = h.image.onload;
  await h.visibility(true);
  assert.equal(h.image.getAttribute("src"), null);
  assert.equal(h.image.onerror, null);
  assert.equal(h.image.onload, null);
  assert.equal(h.timers.size, 0);
  assert.equal(h.element("status-title").textContent, "观看已暂停");
  await h.advance(20000);
  assert.equal(h.requests.length, 1);
  oldOnError();
  oldOnLoad();
  assert.equal(h.timers.size, 0, "stale image events cannot revive a hidden session");
  await h.visibility(false);
  assert.equal(h.requests.length, 2);
  assert.equal(h.connections.length, 2);
  await h.visibility(false);
  await h.advance(2000);
  assert.equal(h.requests.length, 3);
  assert.equal(h.connections.length, 2, "duplicate visibility events do not duplicate streams");
});

test("initially hidden page does not fetch or open MJPEG until visible", async () => {
  const h = harness({ hidden: true });
  await h.advance(10000);
  assert.equal(h.requests.length, 0);
  assert.equal(h.connections.length, 0);
  await h.visibility(false);
  assert.equal(h.requests.length, 1);
  await h.reply(0, healthy());
  assert.equal(h.connections.length, 1);
});

test("server error text is displayed literally, never assigned to innerHTML", async () => {
  const h = harness();
  const payload = '<img src=x onerror="globalThis.pwned=true"><script>alert(1)</script>&';
  await h.reply(0, healthy({ last_error: payload }));
  assert.equal(h.element("last-error").textContent, payload);
  assert.equal(h.element("error-notice").hidden, false);
  assert.equal(h.element("metric-source").textContent, "测试画面");
  assert.equal(h.element("metric-resolution").textContent, "640 × 480");
  assert.equal(h.element("metric-fps").textContent, "9.5 / 10.0 fps");
  assert.equal(h.element("metric-viewers").textContent, "1");
  assert.equal(h.htmlWrites.length, 0);
  h.autoStatus(() => healthy({ source: "rpicam", frame_sequence: 2 }));
  await h.advance(2000);
  assert.equal(h.element("last-error").textContent, "");
  assert.equal(h.element("error-notice").hidden, true);
  assert.equal(h.element("test-label").hidden, true);
  assert.equal(h.element("metric-source").textContent, "设备摄像头");
});

test("invalid status fields cannot inject markup or open a stream; valid polling recovers", async () => {
  const h = harness();
  await h.reply(0, healthy({ width: "<svg onload=alert(1)>" }));
  assert.equal(h.connections.length, 0);
  assert.equal(h.element("metric-resolution").textContent, "—");
  assert.equal(h.element("status-title").textContent, "设备暂时无法连接");
  assert.equal(h.htmlWrites.length, 0);
  h.autoStatus((now) => healthy({ frame_sequence: now }));
  await h.advance(2000);
  await h.advance(1000);
  assert.equal(h.connections.length, 1);
});