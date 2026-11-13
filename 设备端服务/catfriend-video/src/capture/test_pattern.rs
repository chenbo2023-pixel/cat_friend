use crate::stream::Video;
use image::{codecs::jpeg::JpegEncoder, Rgb, RgbImage};
use std::time::Duration;
use tokio::sync::watch;

// 测试图显示移动色块及二进制序号；不依赖本机摄像头或系统字体。
pub fn render(width: u32, height: u32, sequence: u64, quality: u8) -> Result<Vec<u8>, String> {
    let mut image = RgbImage::new(width, height);
    let bar = ((sequence * 13) % u64::from(width)) as u32;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let r = (x * 160 / width + 25) as u8;
        let g = (y * 160 / height + 25) as u8;
        *pixel = if x.abs_diff(bar) < width / 20 + 1 {
            Rgb([245, 190, 65])
        } else {
            Rgb([r, g, 85])
        };
        if y >= height * 3 / 4 && y < height * 7 / 8 {
            let bit = (x * 16 / width).min(15);
            *pixel = if sequence & (1 << bit) != 0 {
                Rgb([240, 240, 240])
            } else {
                Rgb([20, 20, 20])
            };
        }
    }
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(&image)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

pub async fn run(video: &Video, stop: &mut watch::Receiver<bool>) -> Result<(), String> {
    let c = &video.config.capture;
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / f64::from(c.fps)));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sequence = 0;
    loop {
        tokio::select! {
            _ = stop.changed() => return Ok(()),
            _ = tick.tick() => {
                sequence += 1;
                let (width, height, quality) = (c.width, c.height, c.quality);
                let jpeg = tokio::task::spawn_blocking(move || render(width, height, sequence, quality))
                    .await.map_err(|e| e.to_string())??;
                if *stop.borrow() {
                    return Ok(());
                }
                if jpeg.len() > c.max_frame_bytes {
                    return Err("测试帧超过 max_frame_bytes".into());
                }
                video.publish(jpeg.into());
            }
        }
    }
}
