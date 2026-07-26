use prost::Message;

use crate::framing::{FrameDecoder, encode_frame};
use crate::proto::zmk::studio::{Request, Response};

pub fn encode_request(request: &Request) -> Vec<u8> {
    encode_frame(&request.encode_to_vec())
}

/// Feed a chunk of transport bytes into the decoder and return every complete,
/// well-formed response it yields.
///
/// Frames that fail protobuf decoding are dropped: they are remnants of a
/// previous session or of a device-side buffer overflow, and the stream
/// self-heals at the next frame boundary.
pub fn decode_responses(decoder: &mut FrameDecoder, chunk: &[u8]) -> Vec<Response> {
    decoder
        .push(chunk)
        .into_iter()
        .filter_map(|frame| Response::decode(frame.as_slice()).ok())
        .collect()
}
