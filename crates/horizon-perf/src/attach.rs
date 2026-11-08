//! `horizon-perf attach`: adds an `Attachment` to a finished capture on its wire bytes, so fields this build does not know survive.

use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use horizon_perf_format::v1::Attachment;
use prost::Message;

use crate::error::Error;
use crate::follow::{Entry, Follower};
use crate::inflate::GzipInflater;

/// `Capture.attachments`, a length-delimited field.
const ATTACHMENTS_NUMBER: u32 = 22;
const FEED: usize = 64 * 1024;

/// The media type of every attachment the command adds: the body is a JSON document.
pub const JSON_MEDIA_TYPE: &str = "application/json";

/// The gzip bytes of `capture` with `attachment` appended as one more top-level field.
pub fn attach(capture: &[u8], attachment: &Attachment) -> Result<Vec<u8>, Error> {
    let mut follower = Follower::new();
    for slice in capture.chunks(FEED) {
        for entry in follower.push(slice)? {
            if let Entry::Attachment(earlier) = entry
                && earlier.name == attachment.name
            {
                return Err(Error::DuplicateAttachment {
                    name: attachment.name.clone(),
                });
            }
        }
    }
    if follower.sides().next().is_none() {
        return Err(Error::Empty);
    }
    if let Some(&side) = follower.missing_stops().first() {
        return Err(Error::Unfinished { side });
    }
    if !follower.stream_complete() {
        return Err(Error::Corrupt("the gzip stream has no end".into()));
    }
    let mut inflated = Vec::new();
    let mut gz = GzipInflater::default();
    for slice in capture.chunks(FEED) {
        gz.push(slice, &mut inflated)?;
    }
    let body = attachment.encode_to_vec();
    push_varint(&mut inflated, u64::from(ATTACHMENTS_NUMBER << 3 | 2));
    push_varint(&mut inflated, body.len() as u64);
    inflated.extend_from_slice(&body);
    let mut out = GzEncoder::new(Vec::new(), Compression::default());
    out.write_all(&inflated)
        .map_err(|e| Error::Corrupt(e.to_string()))?;
    out.finish().map_err(|e| Error::Corrupt(e.to_string()))
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}
