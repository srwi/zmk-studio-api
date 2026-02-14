use prost::Message;

use crate::framing::{FrameDecoder, FramingError, encode_frame};
use crate::proto::zmk::studio::{Request, Response};

#[derive(Debug)]
pub enum ProtocolError {
    Framing(FramingError),
    Decode(prost::DecodeError),
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Framing(err) => write!(f, "Framing error: {err}"),
            Self::Decode(err) => write!(f, "Decode error: {err}"),
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
