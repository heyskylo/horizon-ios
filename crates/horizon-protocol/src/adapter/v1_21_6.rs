//! Protocol 771 (1.21.6) has the same packet ids and layouts as 772 (1.21.7/1.21.8).
//! Checked against vanilla packets reports and the packet classes in the server jars.
//! Item and sound registry ids differ and are mapped separately in v1_21_6_ids.
pub use super::v1_21_8::{clientbound, serverbound};

pub const PROTOCOL: i32 = 771;
pub const NAME: &str = "1.21.6";
