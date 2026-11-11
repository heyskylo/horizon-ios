use std::fmt;
use std::io::{self, Write};

use flate2::write::GzDecoder;

use crate::v1::{Side, Stop, StopReason, Warning, WarningCode};
use crate::{FORMAT, Field};

/// `Capture.format`'s tag: field 1, varint.
const FORMAT_TAG: u64 = 1 << 3;
const LEN_WIRE: u64 = 2;
/// `Capture.stop`'s number.
const STOP_FIELD: u64 = 23;
/// Every per-side message's `side` tag: field 1, varint.
const SIDE_TAG: u64 = 1 << 3;

/// The most gzip bytes a server may announce for its part: the share endpoint's own upload cap.
pub const MAX_PART_BYTES: u64 = 64 << 20;
/// The most a part may inflate to: room for the timeline's 256 MiB cap (§3.7) and the rest of a 600 s capture.
pub const MAX_PART_INFLATED: u64 = 512 << 20;
/// The longest single top-level field a part may hold, which also bounds what waits half-arrived.
pub const MAX_PART_FIELD: u64 = 64 << 20;

#[derive(Debug)]
pub enum MergeError {
    /// The gzip stream or the protobuf inside it is broken, or it ended early.
    Corrupt(String),
    /// The part is a newer format than this build reads.
    FormatTooNew { found: u64 },
    /// The part inflates past [`MAX_PART_INFLATED`] or holds a field longer than [`MAX_PART_FIELD`].
    TooLarge,
    /// A top-level field of the part is not the server side's, for example the client's or a side-less attachment.
    NotServerSide { field: u64 },
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(why) => f.write_str(why),
            Self::FormatTooNew { found } => {
                write!(
                    f,
                    "the part is format {found}; this build reads up to {FORMAT}"
                )
            }
            Self::TooLarge => write!(
                f,
                "the part is larger than {} MiB inflated or holds a field over {} MiB",
                MAX_PART_INFLATED >> 20,
                MAX_PART_FIELD >> 20
            ),
            Self::NotServerSide { field } => {
                write!(f, "the part holds field {field} of another side")
            }
        }
    }
}

impl std::error::Error for MergeError {}

impl From<io::Error> for MergeError {
    fn from(e: io::Error) -> Self {
        Self::Corrupt(e.to_string())
    }
}

/// Turns another side's finished gzip stream, arriving in any split, into the whole top-level fields to append to this file: its inflated bytes without `format`, untouched otherwise.
pub struct PartMerger {
    gz: GzDecoder<Capped>,
    pending: Vec<u8>,
    yielded: bool,
    stopped: bool,
}

impl Default for PartMerger {
    fn default() -> Self {
        Self::new()
    }
}

impl PartMerger {
    pub fn new() -> Self {
        Self {
            gz: GzDecoder::new(Capped {
                out: Vec::new(),
                left: MAX_PART_INFLATED,
                over: false,
            }),
            pending: Vec::new(),
            yielded: false,
            stopped: false,
        }
    }

    /// What closes a file whose part stopped arriving `t` seconds into the capture: the part's `Stop` if it began but never ended, and the warning on the client side.
    pub fn cut_short(&self, t: f64) -> Vec<Field> {
        let mut fields = Vec::new();
        if self.yielded && !self.stopped {
            fields.push(Field::Stop(Stop {
                side: Side::Server as i32,
                t,
                reason: StopReason::Disconnected as i32,
                overhead: None,
            }));
        }
        fields.push(Field::Warning(Warning {
            side: Side::Client as i32,
            t,
            code: WarningCode::ServerPartIncomplete as i32,
            args: Vec::new(),
        }));
        fields
    }

    /// The next bytes of the part; returns the fields that became whole, ready to append.
    pub fn push(&mut self, gzip: &[u8]) -> Result<Vec<u8>, MergeError> {
        let wrote = self.gz.write_all(gzip).and_then(|()| self.gz.flush());
        if self.gz.get_ref().over {
            return Err(MergeError::TooLarge);
        }
        wrote?;
        self.pending.append(&mut self.gz.get_mut().out);
        let mut out = Vec::new();
        let mut at = 0;
        while let Some((end, format)) = next_field(&self.pending[at..])? {
            match format {
                Some(found) if found > u64::from(FORMAT) => {
                    return Err(MergeError::FormatTooNew { found });
                }
                Some(_) => {}
                None => {
                    let field = &self.pending[at..at + end];
                    if side_of(field) != Some(Side::Server as u64) {
                        let field = varint(field)?.map_or(0, |(tag, _)| tag >> 3);
                        return Err(MergeError::NotServerSide { field });
                    }
                    self.yielded = true;
                    self.stopped |= is_stop(&self.pending[at..at + end]);
                    out.extend_from_slice(&self.pending[at..at + end]);
                }
            }
            at += end;
        }
        self.pending.drain(..at);
        Ok(out)
    }

    /// Checks the part was whole: its gzip trailer arrived and no half field is left.
    pub fn finish(mut self) -> Result<(), MergeError> {
        let finished = self.gz.try_finish();
        if self.gz.get_ref().over {
            return Err(MergeError::TooLarge);
        }
        finished?;
        if self.gz.get_ref().out.is_empty() && self.pending.is_empty() {
            Ok(())
        } else {
            Err(MergeError::Corrupt("the part ends inside a field".into()))
        }
    }
}

/// The inflated bytes not yet taken, refusing more than the part may inflate to.
struct Capped {
    out: Vec<u8>,
    left: u64,
    over: bool,
}

impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(left) = self.left.checked_sub(buf.len() as u64) else {
            self.over = true;
            return Err(io::Error::other("the part inflates past its cap"));
        };
        self.left = left;
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The `side` of a whole top-level field's message; None when it has none.
fn side_of(field: &[u8]) -> Option<u64> {
    let (_, tag_len) = varint(field).ok()??;
    let (_, len_len) = varint(&field[tag_len..]).ok()??;
    let mut rest = &field[tag_len + len_len..];
    while let Some((tag, n)) = varint(rest).ok()? {
        rest = &rest[n..];
        let skip = match tag & 7 {
            0 => {
                let (value, n) = varint(rest).ok()??;
                if tag == SIDE_TAG {
                    return Some(value);
                }
                n
            }
            1 => 8,
            2 => {
                let (len, n) = varint(rest).ok()??;
                n.checked_add(usize::try_from(len).ok()?)?
            }
            5 => 4,
            _ => return None,
        };
        rest = rest.get(skip..)?;
    }
    None
}

fn is_stop(field: &[u8]) -> bool {
    varint(field).is_ok_and(|tag| tag.is_some_and(|(tag, _)| tag >> 3 == STOP_FIELD))
}

/// The length of the first whole field in `bytes` and, for `format`, its value; None while it is incomplete.
fn next_field(bytes: &[u8]) -> Result<Option<(usize, Option<u64>)>, MergeError> {
    let Some((tag, tag_len)) = varint(bytes)? else {
        return Ok(None);
    };
    let rest = &bytes[tag_len..];
    if tag == FORMAT_TAG {
        return Ok(varint(rest)?.map(|(value, n)| (tag_len + n, Some(value))));
    }
    if tag & 7 != LEN_WIRE {
        return Err(MergeError::Corrupt(format!(
            "a top-level field of wire type {}",
            tag & 7
        )));
    }
    let Some((len, len_len)) = varint(rest)? else {
        return Ok(None);
    };
    if len > MAX_PART_FIELD {
        return Err(MergeError::TooLarge);
    }
    let total = usize::try_from(len)
        .ok()
        .and_then(|len| len.checked_add(tag_len + len_len))
        .ok_or_else(|| MergeError::Corrupt("a field longer than memory".into()))?;
    Ok((bytes.len() >= total).then_some((total, None)))
}

/// A protobuf varint at the start of `bytes` and its length; None while its bytes have not all arrived.
fn varint(bytes: &[u8]) -> Result<Option<(u64, usize)>, MergeError> {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().enumerate().take(10) {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(Some((value, i + 1)));
        }
    }
    if bytes.len() >= 10 {
        return Err(MergeError::Corrupt("a varint longer than 10 bytes".into()));
    }
    Ok(None)
}
