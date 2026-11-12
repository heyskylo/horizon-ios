use std::fmt;

use horizon_perf_format::v1::Side;

use crate::wire::SideName;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    NotGzip,
    Corrupt(String),
    FormatTooNew { found: u32, supported: u32 },
    Unfinished { side: Side },
    Empty,
    NoSuchSide { side: Side },
    NoSuchThread { detail: String },
    Incomparable(String),
    DuplicateAttachment { name: String },
}

pub fn side_name(side: Side) -> &'static str {
    SideName::from(side).as_str()
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotGzip => write!(f, "this file is not a .hzperf capture: it is not gzip data"),
            Error::Corrupt(detail) => write!(f, "this capture is damaged: {detail}"),
            Error::FormatTooNew { found, supported } => write!(
                f,
                "this capture is format {found}; this horizon-perf reads up to {supported}: update it"
            ),
            Error::Unfinished { side } => write!(
                f,
                "this capture has no end for its {} side: it is still being written or the game stopped",
                side_name(*side)
            ),
            Error::Empty => write!(f, "this capture has no data yet: it is still being written"),
            Error::NoSuchThread { detail } => f.write_str(detail),
            Error::Incomparable(detail) => f.write_str(detail),
            Error::DuplicateAttachment { name } => {
                write!(f, "this capture already has an attachment named {name}")
            }
            Error::NoSuchSide { side } => {
                write!(f, "this capture has no {} side", side_name(*side))
            }
        }
    }
}

impl std::error::Error for Error {}
