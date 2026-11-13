pub mod jpeg;
mod rpicam;
mod test_pattern;

use crate::{
    config::Source,
    stream::{CaptureState, Video},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

pub async fn run(video: Arc<Video>, mut stop: watch::Receiver<bool>) {
    let mut attempt = 0u32;
    loop {
        if *stop.borrow() {
            break;
        }
        let started = std::time::Instant::now();
        let result = match video.config.capture.source {
            Source::TestPattern => test_pattern::run(&video, &mut stop).await,
            Source::Rpicam => rpicam::run(&video, &mut stop).await,
        };
        if *stop.borrow() {
            break;
        }
        let error = result.err().unwrap_or_else(|| "采集进程已停止".into());
        tracing::warn!(%error, "采集失败，将重试");
        video.set_state(CaptureState::Reconnecting, Some(error));
        if started.elapsed() > Duration::from_secs(30) {
            attempt = 0;
        }
        let delay = Duration::from_secs([1, 2, 5, 10][attempt.min(3) as usize]);
        attempt = attempt.saturating_add(1);
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
    video.set_state(CaptureState::Stopped, None);
}
