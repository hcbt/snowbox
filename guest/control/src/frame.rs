//! Host→Guest Window frames on vsock 53.
//!
//!   u8 type=0, u32le length, stdin bytes
//!   u8 type=1, u16le rows, u16le cols
//!
//! Guest→Host is raw PTY output (no framing).

use std::io::{self, Read};

pub const FRAME_STDIN: u8 = 0;
pub const FRAME_WINSIZE: u8 = 1;

const MAX_STDIN: u32 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFrame {
    Stdin(Vec<u8>),
    Winsize { rows: u16, cols: u16 },
}

pub fn encode_stdin(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + bytes.len());
    out.push(FRAME_STDIN);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    out
}

pub fn encode_winsize(rows: u16, cols: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(FRAME_WINSIZE);
    out.extend_from_slice(&rows.to_le_bytes());
    out.extend_from_slice(&cols.to_le_bytes());
    out
}

/// Pull complete frames off `pending`. Incomplete tails stay in the buffer.
pub fn consume_frames(pending: &mut Vec<u8>) -> io::Result<Vec<HostFrame>> {
    let mut out = Vec::new();
    loop {
        if pending.is_empty() {
            return Ok(out);
        }
        match pending[0] {
            FRAME_STDIN => {
                if pending.len() < 5 {
                    return Ok(out);
                }
                let len = u32::from_le_bytes(pending[1..5].try_into().unwrap());
                if len > MAX_STDIN {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stdin frame too large",
                    ));
                }
                let len = len as usize;
                if pending.len() < 5 + len {
                    return Ok(out);
                }
                out.push(HostFrame::Stdin(pending[5..5 + len].to_vec()));
                pending.drain(..5 + len);
            }
            FRAME_WINSIZE => {
                if pending.len() < 5 {
                    return Ok(out);
                }
                let rows = u16::from_le_bytes(pending[1..3].try_into().unwrap());
                let cols = u16::from_le_bytes(pending[3..5].try_into().unwrap());
                out.push(HostFrame::Winsize { rows, cols });
                pending.drain(..5);
            }
            other => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("bad frame type {other}"),
                ));
            }
        }
    }
}

pub fn copy_stdin_frames(
    stream: &mut impl Read,
    mut on_frame: impl FnMut(HostFrame) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    let mut pending = Vec::new();
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        pending.extend_from_slice(&buf[..n]);
        for frame in consume_frames(&mut pending)? {
            on_frame(frame)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_frame_layout() {
        let f = encode_stdin(b"abc");
        assert_eq!(f[0], FRAME_STDIN);
        assert_eq!(&f[1..5], &3u32.to_le_bytes());
        assert_eq!(&f[5..], b"abc");
    }

    #[test]
    fn winsize_frame_layout() {
        let f = encode_winsize(24, 80);
        assert_eq!(f, vec![FRAME_WINSIZE, 24, 0, 80, 0]);
    }

    #[test]
    fn consume_stdin_and_winsize() {
        let mut pending = encode_stdin(b"hi");
        pending.extend_from_slice(&encode_winsize(24, 80));
        let frames = consume_frames(&mut pending).unwrap();
        assert_eq!(
            frames,
            vec![
                HostFrame::Stdin(b"hi".to_vec()),
                HostFrame::Winsize { rows: 24, cols: 80 },
            ]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn consume_holds_partial() {
        let full = encode_stdin(b"hello");
        let mut pending = full[..4].to_vec();
        let frames = consume_frames(&mut pending).unwrap();
        assert!(frames.is_empty());
        assert_eq!(pending.len(), 4);
        pending.extend_from_slice(&full[4..]);
        let frames = consume_frames(&mut pending).unwrap();
        assert_eq!(frames, vec![HostFrame::Stdin(b"hello".to_vec())]);
        assert!(pending.is_empty());
    }

    #[test]
    fn consume_rejects_unknown_type() {
        let mut pending = vec![99, 0, 0, 0, 0];
        let err = consume_frames(&mut pending).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
