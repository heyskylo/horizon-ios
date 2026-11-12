//! Quiet commands: a client mod's batch of commands that a Horizon server runs
//! as the player who sent them, at that player's own permission level, with
//! no feedback in the chat (docs/mods.md, `command.run-quiet`).
//!
//! The server announces it understands them with an empty [`CHANNEL`] payload
//! in the configuration phase (a vanilla client ignores it); a client that
//! hears it sends each batch as a serverbound [`CHANNEL`] payload, and falls
//! back to chat commands on any other server. A payload is the mod's id, then
//! the lines without their slash.
use crate::{
    Error, Result,
    buf::{Reader, Writer},
};

/// The custom payload channel, both ways.
pub const CHANNEL: &str = "horizon:quiet_commands";
/// Lines in one payload at most; a payload with more is refused whole.
pub const MAX_LINES: usize = 256;
/// Characters in a line at most (a chat command's limit).
pub const MAX_LINE: usize = 256;
/// Characters in a mod id at most.
pub const MAX_MOD_ID: usize = 64;
/// Bytes of a serverbound custom payload's data at most
/// (ServerboundCustomPayloadPacket.MAX_PAYLOAD_SIZE).
pub const MAX_PAYLOAD: usize = 32767;

/// A batch as the server reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub mod_id: String,
    pub lines: Vec<String>,
}

/// `lines` as few payloads as fit [`MAX_PAYLOAD`] and [`MAX_LINES`], in
/// order. Lines longer than [`MAX_LINE`] are cut, as a chat command's are.
pub fn encode(mod_id: &str, lines: &[String]) -> Vec<Vec<u8>> {
    let mod_id: String = mod_id.chars().take(MAX_MOD_ID).collect();
    let mut payloads = Vec::new();
    let mut rest = lines
        .iter()
        .map(|l| -> String { l.chars().take(MAX_LINE).collect() });
    let mut next = rest.next();
    while next.is_some() {
        let mut batch = Vec::new();
        // The id, the count (a varint of at most 2 bytes for 256).
        let mut size = string_size(&mod_id) + 2;
        while let Some(line) = next.take() {
            let more = string_size(&line);
            if !batch.is_empty() && (batch.len() == MAX_LINES || size + more > MAX_PAYLOAD) {
                next = Some(line);
                break;
            }
            size += more;
            batch.push(line);
            next = rest.next();
        }
        let mut w = Writer::new();
        w.string(&mod_id);
        w.varint(batch.len() as i32);
        for line in &batch {
            w.string(line);
        }
        payloads.push(w.data);
    }
    payloads
}

fn string_size(s: &str) -> usize {
    crate::buf::varint_size(s.len() as i32) + s.len()
}

/// A payload's batch; an error for one that breaks a cap.
pub fn decode(data: &[u8]) -> Result<Batch> {
    if data.len() > MAX_PAYLOAD {
        return Err(Error::new("quiet commands: payload too big"));
    }
    let mut r = Reader::new(data);
    // As StringUtil.isAllowedChatCharacter, which a chat command must pass: no control characters
    // (a newline in the mod id would make whole lines of the server's log), DEL or section sign.
    let checked = |s: String| -> Result<String> {
        if s.chars()
            .all(|c| c >= ' ' && c != '\u{7f}' && c != '\u{a7}')
        {
            Ok(s)
        } else {
            Err(Error::new("quiet commands: illegal characters"))
        }
    };
    let mod_id = checked(r.string(MAX_MOD_ID)?)?;
    let count = r.varint()?;
    if !(0..=MAX_LINES as i32).contains(&count) {
        return Err(Error::new(format!("quiet commands: {count} lines")));
    }
    let lines = (0..count)
        .map(|_| r.string(MAX_LINE).and_then(checked))
        .collect::<Result<Vec<_>>>()?;
    if r.remaining() > 0 {
        return Err(Error::new("quiet commands: trailing bytes"));
    }
    Ok(Batch { mod_id, lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_refused_in_the_mod_id_and_lines() {
        let forged = "x\n[12:00:00] [Server thread/INFO]: Stopping server";
        for (id, line) in [
            (forged, "say hi"),
            ("orbital", "say hi\nop Kim"),
            ("orbital", "say \u{a7}cred"),
            ("orb\u{7f}", "say hi"),
        ] {
            let payload = &encode(id, &[line.to_owned()])[0];
            assert!(decode(payload).is_err(), "{id:?} {line:?}");
        }
        let fine = &encode("orbital", &["say hé".to_owned()])[0];
        assert_eq!(decode(fine).unwrap().mod_id, "orbital");
    }

    #[test]
    fn a_batch_round_trips() {
        let lines = vec!["fill 0 0 0 1 1 1 stone".to_string(), "say hi".to_string()];
        let payloads = encode("orbital", &lines);
        assert_eq!(payloads.len(), 1);
        assert_eq!(
            decode(&payloads[0]).unwrap(),
            Batch {
                mod_id: "orbital".into(),
                lines
            }
        );
    }

    #[test]
    fn big_batches_split_by_lines_and_bytes() {
        let lines: Vec<String> = (0..600)
            .map(|i| format!("setblock {i} 0 0 stone"))
            .collect();
        let payloads = encode("m", &lines);
        let decoded: Vec<Batch> = payloads.iter().map(|p| decode(p).unwrap()).collect();
        assert_eq!(
            decoded.iter().map(|b| b.lines.len()).collect::<Vec<_>>(),
            [256, 256, 88]
        );
        assert_eq!(
            decoded
                .into_iter()
                .flat_map(|b| b.lines)
                .collect::<Vec<_>>(),
            lines
        );
        let long: Vec<String> = (0..256).map(|_| "x".repeat(300)).collect();
        for p in encode("m", &long) {
            assert!(p.len() <= MAX_PAYLOAD);
            assert!(
                decode(&p)
                    .unwrap()
                    .lines
                    .iter()
                    .all(|l| l.len() == MAX_LINE)
            );
        }
    }

    #[test]
    fn payloads_over_the_caps_are_refused() {
        let mut w = Writer::new();
        w.string("m");
        w.varint(MAX_LINES as i32 + 1);
        for _ in 0..=MAX_LINES {
            w.string("say");
        }
        assert!(decode(&w.data).is_err());
        let mut w = Writer::new();
        w.string("m");
        w.varint(1);
        w.string(&"x".repeat(MAX_LINE + 1));
        assert!(decode(&w.data).is_err());
        let mut w = Writer::new();
        w.string("m");
        w.varint(-1);
        assert!(decode(&w.data).is_err());
        assert!(decode(&[0xFF]).is_err());
        let mut w = Writer::new();
        w.string("m");
        w.varint(0);
        w.u8(1);
        assert!(decode(&w.data).is_err());
    }
}
