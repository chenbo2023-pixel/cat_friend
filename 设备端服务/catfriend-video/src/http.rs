use crate::stream::Video;
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use bytes::Bytes;
use std::{convert::Infallible, sync::Arc, time::Duration};

pub fn router(video: Arc<Video>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style))
        .route("/player.js", get(player))
        .route("/api/status", get(status))
        .route("/snapshot.jpg", get(snapshot))
        .route("/stream.mjpg", get(mjpeg))
        .with_state(video)
}

fn no_cache(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn index() -> Response {
    asset(
        "text/html; charset=utf-8",
        include_str!("../static/index.html"),
    )
}

async fn style() -> Response {
    asset(
        "text/css; charset=utf-8",
        include_str!("../static/style.css"),
    )
}

async fn player() -> Response {
    asset(
        "text/javascript; charset=utf-8",
        include_str!("../static/player.js"),
    )
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    no_cache(([(header::CONTENT_TYPE, content_type)], body).into_response())
}

async fn status(State(video): State<Arc<Video>>) -> Response {
    no_cache(Json(video.status()).into_response())
}

async fn snapshot(State(video): State<Arc<Video>>) -> Response {
    match video.fresh_frame() {
        Some(frame) => no_cache(
            (
                [
                    ("content-type", "image/jpeg"),
                    ("x-frame-sequence", &frame.sequence.to_string()),
                    ("x-captured-at-ms", &frame.captured_at_ms.to_string()),
                ],
                frame.jpeg.clone(),
            )
                .into_response(),
        ),
        None => no_cache((StatusCode::SERVICE_UNAVAILABLE, "视频源尚无新鲜画面").into_response()),
    }
}

async fn mjpeg(State(video): State<Arc<Video>>) -> Response {
    if video.stopped() {
        return no_cache((StatusCode::SERVICE_UNAVAILABLE, "视频服务已停止").into_response());
    }
    let Some(viewer) = video.acquire_viewer() else {
        return no_cache((StatusCode::TOO_MANY_REQUESTS, "观看连接已达上限").into_response());
    };
    let mut receiver = video.subscribe();
    let stream = async_stream::stream! {
        let _viewer = viewer; // Body 被取消或断连时自动释放名额。
        let mut last_sequence = 0;
        loop {
            let frame = receiver.borrow_and_update().clone();
            if let Some(frame) = frame.filter(|f| video.is_fresh(f) && f.sequence > last_sequence) {
                last_sequence = frame.sequence;
                let header = format!(
                    "--catfriend-frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nX-Frame-Sequence: {}\r\nX-Captured-At-Ms: {}\r\n\r\n",
                    frame.jpeg.len(), frame.sequence, frame.captured_at_ms
                );
                yield Ok::<Bytes, Infallible>(Bytes::from(header));
                yield Ok(Bytes::clone(&frame.jpeg));
                yield Ok(Bytes::from_static(b"\r\n"));
            }
            if video.stopped() {
                break;
            }
            // 无帧时关闭连接，浏览器按退避策略重连，不无限挂住旧画面。
            match tokio::time::timeout(
                Duration::from_millis(video.config.capture.frame_timeout_ms),
                receiver.changed()
            ).await {
                Ok(Ok(())) => {}
                _ => break,
            }
        }
        yield Ok(Bytes::from_static(b"--catfriend-frame--\r\n"));
    };
    no_cache(
        (
            [
                (
                    "content-type",
                    "multipart/x-mixed-replace; boundary=catfriend-frame",
                ),
                ("x-accel-buffering", "no"),
            ],
            Body::from_stream(stream),
        )
            .into_response(),
    )
}
