//! Byte framing for ZMK Studio transport frames.

pub const FRAMING_SOF: u8 = 0xAB;
pub const FRAMING_ESC: u8 = 0xAC;
pub const FRAMING_EOF: u8 = 0xAD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeState {
    Idle,
    AwaitingData,
    Escaped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    ExpectedStartOfFrame,
    UnexpectedStartOfFrameMidFrame,
}

impl core::fmt::Display for FramingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ExpectedStartOfFrame => write!(f, "expected start-of-frame byte"),
            Self::UnexpectedStartOfFrameMidFrame => {
                write!(f, "unexpected start-of-frame mid-frame")
            }
        }
    }
}

impl std::error::Error for FramingError {}

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 2);
    out.push(FRAMING_SOF);

    for &b in payload {
        if matches!(b, FRAMING_SOF | FRAMING_ESC | FRAMING_EOF) {
            out.push(FRAMING_ESC);
        }

        out.push(b);
    }

    out.push(FRAMING_EOF);
    out
}

#[derive(Debug)]
pub struct FrameDecoder {
    state: DecodeState,
    data: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            state: DecodeState::Idle,
            data: Vec::new(),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, FramingError> {
        let mut frames = Vec::new();

        for &b in chunk {
            match self.state {
                DecodeState::Idle => {
                    if b == FRAMING_SOF {
                        self.state = DecodeState::AwaitingData;
                    } else {
                        self.data.clear();
                        self.state = DecodeState::Idle;
                        return Err(FramingError::ExpectedStartOfFrame);
                    }
                }
                DecodeState::AwaitingData => match b {
                    FRAMING_SOF => {
                        self.data.clear();
                        self.state = DecodeState::Idle;
                        return Err(FramingError::UnexpectedStartOfFrameMidFrame);
                    }
                    FRAMING_ESC => {
                        self.state = DecodeState::Escaped;
                    }
                    FRAMING_EOF => {
                        frames.push(core::mem::take(&mut self.data));
                        self.state = DecodeState::Idle;
                    }
                    _ => {
                        self.data.push(b);
                    }
                },
                DecodeState::Escaped => {
                    self.data.push(b);
                    self.state = DecodeState::AwaitingData;
                }
            }
        }

        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameDecoder, encode_frame};

    #[test]
    fn encodes_basic_frame() {
        let input = [1_u8, 2, 3];
        let encoded = encode_frame(&input);
        assert_eq!(encoded, vec![171, 1, 2, 3, 173]);
    }

    #[test]
    fn encodes_escaped_frame() {
        let input = [1_u8, 171, 172, 2, 3, 171, 4, 173, 5];
        let encoded = encode_frame(&input);
        assert_eq!(
            encoded,
            vec![
                171, 1, 172, 171, 172, 172, 2, 3, 172, 171, 4, 172, 173, 5, 173
            ]
        );
    }

    #[test]
    fn decodes_multiple_frames() {
        let input = [171_u8, 1, 2, 3, 173, 171, 4, 173];
        let mut decoder = FrameDecoder::new();
        let frames = decoder.push(&input).expect("decode should succeed");

        assert_eq!(frames, vec![vec![1, 2, 3], vec![4]]);
    }

    #[test]
    fn decodes_escaped_frame_byte_by_byte() {
        let input = [
            171_u8, 1, 172, 171, 172, 172, 2, 3, 172, 171, 4, 172, 173, 5, 173,
        ];

        let mut decoder = FrameDecoder::new();
        let mut frames = Vec::new();

        for &b in &input {
            frames.extend(
                decoder
                    .push(core::slice::from_ref(&b))
                    .expect("decode should succeed"),
            );
        }

        assert_eq!(frames, vec![vec![1, 171, 172, 2, 3, 171, 4, 173, 5]]);
    }
}
