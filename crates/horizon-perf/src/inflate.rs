use flate2::{Crc, Decompress, FlushDecompress, Status};

use crate::error::Error;

const OUT_CHUNK: usize = 64 * 1024;

enum State {
    Header(Vec<u8>),
    Body(Box<Decompress>),
    Trailer(Vec<u8>),
    Done,
}

/// Inflates one gzip member from bytes that arrive in any split, appending whatever is whole so far.
pub struct GzipInflater {
    state: State,
    crc: Crc,
}

impl Default for GzipInflater {
    fn default() -> Self {
        Self {
            state: State::Header(Vec::new()),
            crc: Crc::new(),
        }
    }
}

impl GzipInflater {
    /// True once the trailer has arrived and matched.
    pub fn is_done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// Feeds `bytes` and appends the newly inflated bytes to `out`.
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<(), Error> {
        match &mut self.state {
            State::Header(buf) => {
                buf.extend_from_slice(bytes);
                check_magic(buf)?;
                let Some(len) = header_len(buf) else {
                    return Ok(());
                };
                let rest = buf.split_off(len);
                self.state = State::Body(Box::new(Decompress::new(false)));
                self.push(&rest, out)
            }
            State::Body(inflater) => {
                let start = out.len();
                let mut input = bytes;
                let mut ended = false;
                loop {
                    out.reserve(OUT_CHUNK);
                    let (before_in, before_out) = (inflater.total_in(), inflater.total_out());
                    let status = inflater
                        .decompress_vec(input, out, FlushDecompress::None)
                        .map_err(|e| Error::Corrupt(e.to_string()))?;
                    let used = (inflater.total_in() - before_in) as usize;
                    input = &input[used..];
                    if status == Status::StreamEnd {
                        ended = true;
                        break;
                    }
                    if used == 0 && inflater.total_out() == before_out {
                        break;
                    }
                }
                self.crc.update(&out[start..]);
                if ended {
                    let rest = input.to_vec();
                    self.state = State::Trailer(Vec::new());
                    return self.push(&rest, out);
                }
                Ok(())
            }
            State::Trailer(buf) => {
                buf.extend_from_slice(bytes);
                if buf.len() < 8 {
                    return Ok(());
                }
                if buf.len() > 8 {
                    return Err(Error::Corrupt(
                        "data after the end of the gzip stream".into(),
                    ));
                }
                let crc = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                let size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
                if crc != self.crc.sum() || size != self.crc.amount() {
                    return Err(Error::Corrupt("the gzip checksum does not match".into()));
                }
                self.state = State::Done;
                Ok(())
            }
            State::Done if bytes.is_empty() => Ok(()),
            State::Done => Err(Error::Corrupt(
                "data after the end of the gzip stream".into(),
            )),
        }
    }
}

fn check_magic(buf: &[u8]) -> Result<(), Error> {
    for (i, want) in [0x1f, 0x8b, 8].into_iter().enumerate() {
        if buf.get(i).is_some_and(|&b| b != want) {
            return Err(Error::NotGzip);
        }
    }
    Ok(())
}

/// Length of the RFC 1952 header, or `None` while it is incomplete.
fn header_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 10 {
        return None;
    }
    let flags = buf[3];
    let mut at = 10;
    if flags & 0x04 != 0 {
        let len = u16::from_le_bytes([*buf.get(at)?, *buf.get(at + 1)?]) as usize;
        at += 2 + len;
    }
    for bit in [0x08, 0x10] {
        if flags & bit != 0 {
            at += buf.get(at..)?.iter().position(|&b| b == 0)? + 1;
        }
    }
    if flags & 0x02 != 0 {
        at += 2;
    }
    (buf.len() >= at).then_some(at)
}
