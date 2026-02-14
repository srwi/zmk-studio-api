//! Protocol-level helpers for frame + protobuf handling.

use prost::Message;

use crate::framing::{encode_frame, FrameDecoder, FramingError};
use crate::proto::zmk::studio::{Request, Response};

#[derive(Debug)]
pub enum ProtocolError {
    Framing(FramingError),
    Decode(prost::DecodeError),
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Framing(err) => write!(f, "framing error: {err}"),
            Self::Decode(err) => write!(f, "decode error: {err}"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Framing(err) => Some(err),
            Self::Decode(err) => Some(err),
        }
    }
}

impl From<FramingError> for ProtocolError {
    fn from(value: FramingError) -> Self {
        Self::Framing(value)
    }
}

impl From<prost::DecodeError> for ProtocolError {
    fn from(value: prost::DecodeError) -> Self {
        Self::Decode(value)
    }
}

pub fn encode_request(request: &Request) -> Vec<u8> {
    encode_frame(&request.encode_to_vec())
}

pub fn encode_response(response: &Response) -> Vec<u8> {
    encode_frame(&response.encode_to_vec())
}

pub fn decode_requests(
    decoder: &mut FrameDecoder,
    chunk: &[u8],
) -> Result<Vec<Request>, ProtocolError> {
    decoder
        .push(chunk)?
        .into_iter()
        .map(|frame| Request::decode(frame.as_slice()).map_err(ProtocolError::from))
        .collect()
}

pub fn decode_responses(
    decoder: &mut FrameDecoder,
    chunk: &[u8],
) -> Result<Vec<Response>, ProtocolError> {
    decoder
        .push(chunk)?
        .into_iter()
        .map(|frame| Response::decode(frame.as_slice()).map_err(ProtocolError::from))
        .collect()
}