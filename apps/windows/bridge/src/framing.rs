//! Chrome native-messaging framing: a 32-bit length in native byte order
//! (little-endian on every Windows target) followed by that many bytes of
//! UTF-8 JSON. One message in, one message out per process.

use std::io::{self, Read, Write};

/// Session keys are a few hundred bytes; anything near this is not ours, so a
/// length above it is rejected before a single byte of the body is read.
pub const MAX_INBOUND: u32 = 1024 * 1024;

#[derive(Debug)]
pub enum FramingError {
    /// Stdin closed before any byte of the length: the browser disconnected.
    Eof,
    TooLarge(u32),
    Truncated,
    Io(io::Error),
}

/// Reads one framed message. Returns [`FramingError::Eof`] when stdin is
/// closed before the message begins (a clean disconnect).
pub fn read_message<R: Read>(r: &mut R) -> Result<Vec<u8>, FramingError> {
    let mut len = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut len[got..]) {
            Ok(0) if got == 0 => return Err(FramingError::Eof),
            Ok(0) => return Err(FramingError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FramingError::Io(e)),
        }
    }
    let len = u32::from_le_bytes(len);
    if len > MAX_INBOUND {
        return Err(FramingError::TooLarge(len));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FramingError::Truncated,
        _ => FramingError::Io(e),
    })?;
    Ok(body)
}

/// Writes one framed message: the 32-bit length then the body.
pub fn write_message<W: Write>(w: &mut W, body: &[u8]) -> io::Result<()> {
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(body)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &[u8]) -> Vec<u8> {
        [&(body.len() as u32).to_le_bytes()[..], body].concat()
    }

    #[test]
    fn reads_one_message() {
        let mut input = Cursor::new(framed(br#"{"a":1}"#));
        assert_eq!(read_message(&mut input).unwrap(), br#"{"a":1}"#);
    }

    #[test]
    fn empty_stdin_is_eof() {
        assert!(matches!(
            read_message(&mut Cursor::new(Vec::new())),
            Err(FramingError::Eof)
        ));
    }

    #[test]
    fn oversized_length_is_rejected() {
        let mut input = Cursor::new((MAX_INBOUND + 1).to_le_bytes().to_vec());
        assert!(matches!(
            read_message(&mut input),
            Err(FramingError::TooLarge(n)) if n == MAX_INBOUND + 1
        ));
    }

    #[test]
    fn truncated_body_is_an_error() {
        let mut bytes = 10u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"abc");
        assert!(matches!(
            read_message(&mut Cursor::new(bytes)),
            Err(FramingError::Truncated)
        ));
    }

    #[test]
    fn truncated_length_is_an_error() {
        assert!(matches!(
            read_message(&mut Cursor::new(vec![1u8, 0])),
            Err(FramingError::Truncated)
        ));
    }

    #[test]
    fn writes_length_then_body() {
        let mut out = Vec::new();
        write_message(&mut out, br#"{"ok":true}"#).unwrap();
        assert_eq!(out, framed(br#"{"ok":true}"#));
    }
}
