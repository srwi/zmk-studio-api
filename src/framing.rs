pub const FRAMING_SOF: u8 = 0xAB;
pub const FRAMING_ESC: u8 = 0xAC;
pub const FRAMING_EOF: u8 = 0xAD;

/// Upper bound for a single frame. Real ZMK Studio messages are far smaller; a
/// frame that grows past this lost its EOF byte, so the decoder discards it and
/// resynchronizes on the next start-of-frame.
const MAX_FRAME_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeState {
    Idle,
    AwaitingData,
    Escaped,
}

/// Streaming frame decoder for the ZMK Studio framing protocol.
///
/// Mirrors the resynchronization behavior of the firmware decoder
/// (`app/src/studio/msg_framing.c` in ZMK): bytes outside a frame are
/// discarded until a start-of-frame is seen, and an unescaped start-of-frame
/// mid-frame drops the partial frame and starts a new one. Decoding never
/// fails; garbage on the wire (stale ring-buffer content from a previous
/// session, truncated frames after a device-side buffer overflow) is skipped
/// instead of poisoning the connection.
#[derive(Debug)]
pub struct FrameDecoder {
    state: DecodeState,
    data: Vec<u8>,
    discarded_bytes: u64,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self {
            state: DecodeState::Idle,
            data: Vec::new(),
            discarded_bytes: 0,
        }
    }

    /// Number of bytes skipped so far while resynchronizing (garbage outside
    /// frames and dropped partial frames).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn discarded_bytes(&self) -> u64 {
        self.discarded_bytes
    }

    /// Drop any partial frame and return to the idle state.
    pub fn reset(&mut self) {
        self.discarded_bytes += self.data.len() as u64;
        self.data.clear();
        self.state = DecodeState::Idle;
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();

        for &b in chunk {
            match self.state {
                DecodeState::Idle => {
                    if b == FRAMING_SOF {
                        self.state = DecodeState::AwaitingData;
                    } else {
                        self.discarded_bytes += 1;
                    }
                }
                DecodeState::AwaitingData => match b {
                    FRAMING_SOF => {
                        // Unescaped SOF mid-frame: the current frame was
                        // truncated. Drop it and treat this byte as the start
                        // of a new frame.
                        self.discarded_bytes += self.data.len() as u64;
                        self.data.clear();
                    }
                    FRAMING_ESC => {
                        self.state = DecodeState::Escaped;
                    }
                    FRAMING_EOF => {
                        frames.push(core::mem::take(&mut self.data));
                        self.state = DecodeState::Idle;
                    }
                    _ => {
                        self.push_data_byte(b);
                    }
                },
                DecodeState::Escaped => {
                    self.push_data_byte(b);
                    self.state = DecodeState::AwaitingData;
                }
            }
        }

        frames
    }

    fn push_data_byte(&mut self, b: u8) {
        if self.data.len() >= MAX_FRAME_SIZE {
            self.reset();
            return;
        }
        self.data.push(b);
    }
}

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

#[cfg(test)]
mod tests {
    use super::{FRAMING_SOF, FrameDecoder, encode_frame};

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
        let frames = decoder.push(&input);

        assert_eq!(frames, vec![vec![1, 2, 3], vec![4]]);
        assert_eq!(decoder.discarded_bytes(), 0);
    }

    #[test]
    fn decodes_escaped_frame_byte_by_byte() {
        let input = [
            171_u8, 1, 172, 171, 172, 172, 2, 3, 172, 171, 4, 172, 173, 5, 173,
        ];

        let mut decoder = FrameDecoder::new();
        let mut frames = Vec::new();

        for &b in &input {
            frames.extend(decoder.push(core::slice::from_ref(&b)));
        }

        assert_eq!(frames, vec![vec![1, 171, 172, 2, 3, 171, 4, 173, 5]]);
    }

    #[test]
    fn skips_garbage_before_frame() {
        // Stale bytes from a previous session (e.g. the tail of a truncated
        // response) must not prevent decoding of the frame that follows.
        let input = [7_u8, 8, 9, 171, 1, 2, 173];
        let mut decoder = FrameDecoder::new();
        let frames = decoder.push(&input);

        assert_eq!(frames, vec![vec![1, 2]]);
        assert_eq!(decoder.discarded_bytes(), 3);
    }

    #[test]
    fn unescaped_sof_starts_new_frame() {
        // A frame that lost its EOF (device-side buffer overflow) is dropped
        // when the next frame starts.
        let input = [171_u8, 1, 2, 171, 3, 4, 173];
        let mut decoder = FrameDecoder::new();
        let frames = decoder.push(&input);

        assert_eq!(frames, vec![vec![3, 4]]);
        assert_eq!(decoder.discarded_bytes(), 2);
    }

    #[test]
    fn reset_drops_partial_frame() {
        let mut decoder = FrameDecoder::new();
        assert!(decoder.push(&[171, 1, 2]).is_empty());
        decoder.reset();

        let frames = decoder.push(&[171, 9, 173]);
        assert_eq!(frames, vec![vec![9]]);
    }

    #[test]
    fn oversized_frame_is_dropped() {
        let mut decoder = FrameDecoder::new();
        let mut input = vec![FRAMING_SOF];
        input.extend(std::iter::repeat_n(1_u8, super::MAX_FRAME_SIZE + 8));
        assert!(decoder.push(&input).is_empty());

        // Decoder resynchronizes on the next frame.
        let frames = decoder.push(&[171, 5, 173]);
        assert_eq!(frames, vec![vec![5]]);
    }
}
