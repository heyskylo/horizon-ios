use horizon_perf_format::v1::Capture;

use crate::error::Error;
use crate::follow::Follower;

const SLICE: usize = 64 * 1024;

/// Reads a whole finished capture; a file whose sides lack a `Stop` is refused, never salvaged.
pub fn read_capture(bytes: &[u8]) -> Result<Capture, Error> {
    let mut follower = Follower::new();
    let mut capture = Capture::default();
    for slice in bytes.chunks(SLICE) {
        for entry in follower.push(slice)? {
            entry.apply(&mut capture);
        }
    }
    if follower.sides().next().is_none() {
        return Err(Error::Empty);
    }
    if let Some(&side) = follower.missing_stops().first() {
        return Err(Error::Unfinished { side });
    }
    Ok(capture)
}
