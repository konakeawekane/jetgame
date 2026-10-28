//! Bounded, versioned control messages. Gameplay transport is intentionally separate.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};

pub const VERSION: u16 = 1;
pub const MAX_FRAME: usize = 65536;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub version: u16,
    pub kind: String,
    pub request_id: u64,
    pub payload: Value,
}

impl Message {
    pub fn new(kind: &str, payload: Value) -> Self {
        Self {
            version: VERSION,
            kind: kind.into(),
            request_id: 0,
            payload,
        }
    }
    fn validate(&self) -> io::Result<()> {
        if self.version != VERSION
            || self.kind.is_empty()
            || self.kind.len() > 40
            || !self.payload.is_object()
        {
            return Err(invalid("unsupported protocol version or invalid envelope"));
        }
        Ok(())
    }
}

pub fn encode(message: &Message) -> io::Result<Vec<u8>> {
    message.validate()?;
    let body = serde_json::to_vec(message).map_err(invalid)?;
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(invalid("control frame too large"));
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn decode(body: &[u8]) -> io::Result<Message> {
    let message: Message = serde_json::from_slice(body).map_err(invalid)?;
    message.validate()?;
    Ok(message)
}

pub fn exchange(stream: &mut TcpStream, message: &Message) -> io::Result<Message> {
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&encode(message)?)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut prefix = [0_u8; 4];
    read_deadline(stream, &mut prefix, deadline)?;
    let size = u32::from_be_bytes(prefix) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(invalid("invalid frame size"));
    }
    let mut body = vec![0; size];
    read_deadline(stream, &mut body, deadline)?;
    decode(&body)
}

fn read_deadline(
    stream: &mut TcpStream,
    mut buffer: &mut [u8],
    deadline: Instant,
) -> io::Result<()> {
    while !buffer.is_empty() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "control response timed out"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(buffer)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        buffer = &mut buffer[count..];
    }
    Ok(())
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}
impl Decoder {
    pub fn push(&mut self, data: &[u8]) -> io::Result<()> {
        if self.buffer.len() + data.len() > MAX_FRAME * 2 + 8 {
            return Err(invalid("receive buffer limit exceeded"));
        }
        self.buffer.extend_from_slice(data);
        Ok(())
    }
    pub fn pop(&mut self) -> io::Result<Option<Message>> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let size = u32::from_be_bytes(self.buffer[..4].try_into().unwrap()) as usize;
        if size == 0 || size > MAX_FRAME {
            return Err(invalid("invalid frame size"));
        }
        if self.buffer.len() < size + 4 {
            return Ok(None);
        }
        let message = decode(&self.buffer[4..size + 4])?;
        self.buffer.drain(..size + 4);
        Ok(Some(message))
    }
}

pub fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn fragmented_and_coalesced_frames_decode() {
        let frame = encode(&Message::new("ping", json!({}))).unwrap();
        let mut decoder = Decoder::default();
        decoder.push(&frame[..3]).unwrap();
        assert!(decoder.pop().unwrap().is_none());
        decoder.push(&frame[3..]).unwrap();
        decoder.push(&frame).unwrap();
        assert_eq!(decoder.pop().unwrap().unwrap().kind, "ping");
        assert_eq!(decoder.pop().unwrap().unwrap().kind, "ping");
        assert!(decoder.pop().unwrap().is_none());
    }
    #[test]
    fn incompatible_versions_and_oversized_frames_fail() {
        let mut message = Message::new("ping", json!({}));
        message.version = 99;
        assert!(encode(&message).is_err());
        let mut decoder = Decoder::default();
        decoder
            .push(&((MAX_FRAME + 1) as u32).to_be_bytes())
            .unwrap();
        assert!(decoder.pop().is_err());
    }
}
