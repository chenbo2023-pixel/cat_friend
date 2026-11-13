//! 增量 JPEG 分帧：跳过带长度的标记段，扫描熵编码中的转义/重启标记。
//! APP/COM 元数据里的 FF D9 不应被误认为帧尾。缓冲大小始终有上限。

#[derive(Clone, Copy)]
enum Mode {
    Seeking,
    Marker,
    Segment { entropy_after: bool },
    Entropy,
}

pub struct Parser {
    bytes: Vec<u8>,
    position: usize,
    mode: Mode,
    limit: usize,
}

impl Parser {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            position: 0,
            mode: Mode::Seeking,
            limit,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut frames = Vec::new();
        // 输入块可能跨多个帧，逐字节处理以保持严格的内存上限。
        for byte in chunk {
            if self.bytes.len() >= self.limit {
                self.reset();
                return Err("JPEG 帧超过 max_frame_bytes，已清空解析缓冲".into());
            }
            self.bytes.push(*byte);
            while self.advance(&mut frames)? {}
        }
        Ok(frames)
    }

    fn reset(&mut self) {
        self.bytes.clear();
        self.position = 0;
        self.mode = Mode::Seeking;
    }

    fn advance(&mut self, frames: &mut Vec<Vec<u8>>) -> Result<bool, String> {
        match self.mode {
            Mode::Seeking => {
                if self.bytes.len() < 2 {
                    return Ok(false);
                }
                if self.bytes[..2] == [0xff, 0xd8] {
                    self.position = 2;
                    self.mode = Mode::Marker;
                    return Ok(true);
                }
                let last = *self.bytes.last().unwrap();
                self.bytes.clear();
                if last == 0xff {
                    self.bytes.push(last);
                }
                Ok(false)
            }
            Mode::Segment { entropy_after } => {
                let p = self.position;
                if self.bytes.len() < p + 2 {
                    return Ok(false);
                }
                let length = usize::from(u16::from_be_bytes([self.bytes[p], self.bytes[p + 1]]));
                if length < 2 || p + length > self.limit {
                    self.reset();
                    return Err("无效的 JPEG 标记段长度".into());
                }
                if self.bytes.len() < p + length {
                    return Ok(false);
                }
                self.position = p + length;
                self.mode = if entropy_after {
                    Mode::Entropy
                } else {
                    Mode::Marker
                };
                Ok(true)
            }
            Mode::Marker | Mode::Entropy => {
                let p = self.position;
                if self.bytes.len() <= p {
                    return Ok(false);
                }
                if self.bytes[p] != 0xff {
                    if matches!(self.mode, Mode::Entropy) {
                        self.position += 1;
                        return Ok(true);
                    }
                    self.reset();
                    return Err("JPEG 标记段损坏".into());
                }
                if self.bytes.len() <= p + 1 {
                    return Ok(false);
                }
                let marker = self.bytes[p + 1];
                match marker {
                    0xff => self.position += 1,
                    0x00 | 0xd0..=0xd7 if matches!(self.mode, Mode::Entropy) => {
                        self.position += 2;
                    }
                    0xd9 => {
                        frames.push(std::mem::take(&mut self.bytes));
                        self.position = 0;
                        self.mode = Mode::Seeking;
                    }
                    0xd8 => {
                        self.bytes.drain(..p);
                        self.position = 2;
                        self.mode = Mode::Marker;
                    }
                    0x01 => self.position += 2,
                    0x00 | 0xd0..=0xd7 => {
                        self.reset();
                        return Err("JPEG 标记无效".into());
                    }
                    _ => {
                        self.position += 2;
                        self.mode = Mode::Segment {
                            entropy_after: marker == 0xda,
                        };
                    }
                }
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::test_pattern::render;

    #[test]
    fn handles_arbitrary_chunk_boundaries_and_marker_bytes_in_metadata() {
        let original = render(64, 48, 1, 75).unwrap();
        let mut jpeg = original[..2].to_vec();
        jpeg.extend_from_slice(&[0xff, 0xfe, 0x00, 0x06, 0xff, 0xd9, 0xff, 0xd8]);
        jpeg.extend_from_slice(&original[2..]);
        let mut input = b"camera noise".to_vec();
        input.extend_from_slice(&jpeg);
        input.extend_from_slice(&original);
        for size in [1, 2, 3, 7, 1024, input.len()] {
            let mut parser = Parser::new(65536);
            let mut frames = Vec::new();
            for chunk in input.chunks(size) {
                frames.extend(parser.push(chunk).unwrap());
            }
            assert_eq!(frames, vec![jpeg.clone(), original.clone()]);
            assert!(image::load_from_memory(&frames[0]).is_ok());
        }
    }

    #[test]
    fn limits_corrupt_frames_and_recovers_on_next_input() {
        let mut parser = Parser::new(1024);
        assert!(parser.push(&[0xff, 0xd8, 0xff, 0xe0, 0xff, 0xff]).is_err());
        let jpeg = render(16, 16, 1, 50).unwrap();
        assert_eq!(parser.push(&jpeg).unwrap(), vec![jpeg]);
        let mut oversize = vec![0xff, 0xd8, 0xff, 0xda, 0, 2];
        oversize.extend(std::iter::repeat_n(1, 1024));
        assert!(parser.push(&oversize).is_err());
        assert!(parser.bytes.len() <= 1024);
    }
}
