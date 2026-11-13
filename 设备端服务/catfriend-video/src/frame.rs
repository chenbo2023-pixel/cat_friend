use bytes::Bytes;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct Frame {
    pub sequence: u64,
    pub captured_at_ms: u64,
    pub captured_at: Instant,
    pub jpeg: Bytes,
}

impl Frame {
    pub fn new(sequence: u64, jpeg: Bytes) -> Self {
        Self {
            sequence,
            captured_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            captured_at: Instant::now(),
            jpeg,
        }
    }
}
