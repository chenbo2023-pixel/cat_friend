use super::jpeg::Parser;
use crate::stream::Video;
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, sync::watch};

pub async fn run(video: &Video, stop: &mut watch::Receiver<bool>) -> Result<(), String> {
    let c = &video.config.capture;
    // 参数逐项传递给进程，不经过 shell；相机型号/IP 不参与采集实现。
    let mut child = Command::new(&c.binary)
        .args([
            "--nopreview",
            "--timeout",
            "0",
            "--codec",
            "mjpeg",
            "--camera",
            &c.camera.to_string(),
            "--width",
            &c.width.to_string(),
            "--height",
            &c.height.to_string(),
            "--framerate",
            &c.fps.to_string(),
            "--quality",
            &c.quality.to_string(),
            "--output",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("无法启动 {}：{e}", c.binary))?;
    let mut stdout = child.stdout.take().ok_or("采集 stdout 不可用")?;
    let mut stderr = child.stderr.take().ok_or("采集 stderr 不可用")?;
    let diagnostic = Arc::new(Mutex::new(String::new()));
    let diagnostic_writer = diagnostic.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buffer = [0; 1024];
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buffer[..n]).trim().to_owned();
            *diagnostic_writer.lock().unwrap() = text;
        }
    });

    let mut parser = Parser::new(c.max_frame_bytes);
    let mut buffer = [0; 16384];
    let mut deadline = tokio::time::Instant::now() + c.frame_timeout();
    let result = 'capture: loop {
        tokio::select! {
            _ = stop.changed() => break Ok(()),
            _ = tokio::time::sleep_until(deadline) => {
                break Err("采集超时：未收到完整 JPEG 帧".into());
            }
            result = stdout.read(&mut buffer) => {
                let n = match result {
                    Ok(0) => break Err("采集进程关闭了视频输出".into()),
                    Ok(n) => n,
                    Err(e) => break Err(format!("读取采集输出失败：{e}")),
                };
                let frames = match parser.push(&buffer[..n]) {
                    Ok(frames) => frames,
                    Err(e) => break Err(e),
                };
                for jpeg in frames {
                    // 只读取 JPEG 头验证尺寸，避免错误输入伪造监控状态。
                    let dimensions = image::ImageReader::with_format(
                        std::io::Cursor::new(&jpeg), image::ImageFormat::Jpeg
                    ).into_dimensions();
                    if !matches!(dimensions, Ok((width, height)) if width == c.width && height == c.height) {
                        break 'capture Err("JPEG 尺寸与配置不一致或图像头损坏".into());
                    }
                    if *stop.borrow() {
                        break 'capture Ok(());
                    }
                    video.publish(jpeg.into());
                    deadline = tokio::time::Instant::now() + c.frame_timeout();
                }
            }
        }
    };
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    stderr_task.abort();
    result.map_err(|error| {
        let diagnostic = diagnostic.lock().unwrap();
        if diagnostic.is_empty() {
            error
        } else {
            format!("{error}；{}", diagnostic.trim())
        }
    })
}
