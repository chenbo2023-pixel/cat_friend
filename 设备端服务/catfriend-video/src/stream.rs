use crate::{config::Config, frame::Frame};
use bytes::Bytes;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Starting,
    Streaming,
    Reconnecting,
    Stopped,
}

struct CaptureInfo {
    state: CaptureState,
    sequence: u64,
    frame_times: VecDeque<Instant>,
    last_error: Option<String>,
}

pub struct Video {
    pub config: Config,
    latest: watch::Sender<Option<Arc<Frame>>>,
    info: Mutex<CaptureInfo>,
    viewers: AtomicUsize,
}

#[derive(Serialize)]
pub struct Status {
    source: crate::config::Source,
    state: CaptureState,
    healthy: bool,
    frame_sequence: Option<u64>,
    frame_age_ms: Option<u64>,
    width: u32,
    height: u32,
    configured_fps: u32,
    measured_fps: f64,
    viewers: usize,
    last_error: Option<String>,
}

impl Video {
    pub fn new(config: Config) -> Arc<Self> {
        let (latest, _) = watch::channel(None);
        Arc::new(Self {
            config,
            latest,
            info: Mutex::new(CaptureInfo {
                state: CaptureState::Starting,
                sequence: 0,
                frame_times: VecDeque::new(),
                last_error: None,
            }),
            viewers: AtomicUsize::new(0),
        })
    }

    pub fn publish(&self, jpeg: Bytes) {
        let mut info = self.info.lock().unwrap();
        if matches!(info.state, CaptureState::Stopped) {
            return;
        }
        info.sequence += 1;
        let frame = Arc::new(Frame::new(info.sequence, jpeg));
        let now = frame.captured_at;
        info.frame_times.push_back(now);
        while info.frame_times.len() > 150
            || info
                .frame_times
                .front()
                .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(5))
        {
            info.frame_times.pop_front();
        }
        info.state = CaptureState::Streaming;
        info.last_error = None;
        self.latest.send_replace(Some(frame));
    }

    pub fn set_state(&self, state: CaptureState, error: Option<String>) {
        let mut info = self.info.lock().unwrap();
        info.state = state;
        info.last_error = error;
        info.frame_times.clear();
        // 清除旧图，截图和新观看连接不能把故障前画面当作实时帧。
        self.latest.send_replace(None);
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<Frame>>> {
        self.latest.subscribe()
    }

    pub fn fresh_frame(&self) -> Option<Arc<Frame>> {
        self.latest.borrow().clone().filter(|f| self.is_fresh(f))
    }

    pub fn is_fresh(&self, frame: &Frame) -> bool {
        frame.captured_at.elapsed() < Duration::from_millis(self.config.capture.stale_after_ms)
    }

    pub fn stopped(&self) -> bool {
        matches!(self.info.lock().unwrap().state, CaptureState::Stopped)
    }

    pub fn status(&self) -> Status {
        let info = self.info.lock().unwrap();
        let latest = self.latest.borrow();
        let frame_age_ms = latest
            .as_ref()
            .map(|f| f.captured_at.elapsed().as_millis() as u64);
        let measured_fps = match (info.frame_times.front(), info.frame_times.back()) {
            (Some(first), Some(last))
                if last > first
                    && frame_age_ms.is_some_and(|age| age < self.config.capture.stale_after_ms) =>
            {
                (info.frame_times.len() - 1) as f64 / last.duration_since(*first).as_secs_f64()
            }
            _ => 0.0,
        };
        Status {
            source: self.config.capture.source,
            state: info.state,
            healthy: matches!(info.state, CaptureState::Streaming)
                && frame_age_ms.is_some_and(|age| age < self.config.capture.stale_after_ms),
            frame_sequence: latest.as_ref().map(|f| f.sequence),
            frame_age_ms,
            width: self.config.capture.width,
            height: self.config.capture.height,
            configured_fps: self.config.capture.fps,
            measured_fps,
            viewers: self.viewers.load(Ordering::Relaxed),
            last_error: info.last_error.clone(),
        }
    }

    pub fn acquire_viewer(self: &Arc<Self>) -> Option<Viewer> {
        self.viewers
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.config.server.max_viewers).then_some(n + 1)
            })
            .ok()
            .map(|_| Viewer(self.clone()))
    }
}

pub struct Viewer(Arc<Video>);

impl Drop for Viewer {
    fn drop(&mut self) {
        self.0.viewers.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_consumers_receive_latest_frame_and_recovery_clears_old_picture() {
        let video = Video::new(Config::default());
        let receiver = video.subscribe();
        for n in 0..100 {
            video.publish(Bytes::from(vec![n]));
        }
        assert_eq!(receiver.borrow().as_ref().unwrap().sequence, 100);
        assert!(video.status().healthy);
        video.set_state(CaptureState::Reconnecting, Some("camera exited".into()));
        assert!(receiver.borrow().is_none());
        assert!(video.fresh_frame().is_none());
        assert!(!video.status().healthy);
        video.publish(Bytes::from_static(b"new"));
        assert_eq!(video.fresh_frame().unwrap().sequence, 101);
        video.set_state(CaptureState::Stopped, None);
        video.publish(Bytes::from_static(b"late frame"));
        assert!(video.fresh_frame().is_none());
        assert!(video.stopped());
    }

    #[test]
    fn viewers_are_limited_and_released_on_drop() {
        let mut config = Config::default();
        config.server.max_viewers = 1;
        let video = Video::new(config);
        let first = video.acquire_viewer().unwrap();
        assert!(video.acquire_viewer().is_none());
        drop(first);
        assert!(video.acquire_viewer().is_some());
    }
}
