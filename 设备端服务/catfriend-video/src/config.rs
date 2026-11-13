use serde::Deserialize;
use std::{net::SocketAddr, path::Path, time::Duration};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub server: Server,
    pub capture: Capture,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
    pub listen: SocketAddr,
    pub max_viewers: usize,
    pub write_timeout_ms: u64,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8090".parse().unwrap(),
            max_viewers: 4,
            write_timeout_ms: 10000,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    TestPattern,
    Rpicam,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capture {
    pub source: Source,
    pub binary: String,
    pub camera: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub quality: u8,
    pub max_frame_bytes: usize,
    pub stale_after_ms: u64,
    pub frame_timeout_ms: u64,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            source: Source::TestPattern,
            binary: "rpicam-vid".into(),
            camera: 0,
            width: 640,
            height: 480,
            fps: 5,
            quality: 75,
            max_frame_bytes: 4 * 1024 * 1024,
            stale_after_ms: 5000,
            frame_timeout_ms: 15000,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        config.validate().map_err(std::io::Error::other)?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let ip = self.server.listen.ip();
        let private = match ip {
            std::net::IpAddr::V4(ip) => ip.is_private(),
            std::net::IpAddr::V6(ip) => ip.is_unique_local(),
        };
        if !ip.is_loopback() && !private {
            return Err("listen 必须是回环或私网地址；服务依赖 WireGuard 准入".into());
        }
        if self.server.listen.port() == 0 || !(1..=16).contains(&self.server.max_viewers) {
            return Err("端口不能为 0，max_viewers 必须在 1..=16".into());
        }
        if !(1000..=60000).contains(&self.server.write_timeout_ms) {
            return Err("write_timeout_ms 必须在 1000..=60000".into());
        }
        let c = &self.capture;
        if !(16..=1920).contains(&c.width)
            || !(16..=1080).contains(&c.height)
            || c.width % 2 != 0
            || c.height % 2 != 0
        {
            return Err("画面宽高必须是偶数，范围为 16..=1920 × 16..=1080".into());
        }
        if !(1..=30).contains(&c.fps) || !(1..=100).contains(&c.quality) {
            return Err("fps 必须在 1..=30，quality 必须在 1..=100".into());
        }
        if !(64 * 1024..=16 * 1024 * 1024).contains(&c.max_frame_bytes) {
            return Err("max_frame_bytes 必须在 64KiB..=16MiB".into());
        }
        if c.stale_after_ms < 3000 / u64::from(c.fps)
            || c.stale_after_ms > 60000
            || c.frame_timeout_ms < c.stale_after_ms
            || c.frame_timeout_ms > 120000
        {
            return Err(
                "帧过期时间需至少覆盖三个帧间隔；采集超时不得小于过期时间，最大 120 秒".into(),
            );
        }
        if c.binary.trim().is_empty() {
            return Err("rpicam binary 不能为空".into());
        }
        Ok(())
    }
}

impl Capture {
    pub fn frame_timeout(&self) -> Duration {
        Duration::from_millis(self.frame_timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_public_binding_and_resource_abuse() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());
        config.server.listen = "0.0.0.0:8090".parse().unwrap();
        assert!(config.validate().is_err());
        config.server.listen = "10.0.0.3:8090".parse().unwrap();
        config.capture.width = u32::MAX;
        assert!(config.validate().is_err());
        config.capture.width = 640;
        config.capture.fps = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_misspelled_configuration() {
        assert!(toml::from_str::<Config>("[capture]\nframerate = 5").is_err());
    }
}
