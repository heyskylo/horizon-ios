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

#[derive(Debug)]
pub enum MergeError {
    /// The gzip stream or the protobuf inside it is broken, or it ended early.
    Corrupt(String),
    /// The part is a newer format than this build reads.
    FormatTooNew { found: u64 },
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
    gz: GzDecoder<Vec<u8>>,
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
            gz: GzDecoder::new(Vec::new()),
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
        self.gz.write_all(gzip)?;
        self.gz.flush()?;
        self.pending.append(self.gz.get_mut());
        let mut out = Vec::new();
        let mut at = 0;
        while let Some((end, format)) = next_field(&self.pending[at..])? {
            match format {
                Some(found) if found > u64::from(FORMAT) => {
                    return Err(MergeError::FormatTooNew { found });
                }
                Some(_) => {}
                None => {
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
        self.gz.try_finish()?;
        if self.gz.get_mut().is_empty() && self.pending.is_empty() {
            Ok(())
        } else {
            Err(MergeError::Corrupt("the part ends inside a field".into()))
        }
    }
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
