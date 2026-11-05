//! Complete, opt-in raw play receipts for the development command oracle.
//! No packet filtering, UUID replacement, re-encoding or Studio policy changes.
use serde_json::json;
use std::{
    cell::RefCell,
    fs::File,
    io::{self, Write},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
struct Trace {
    file: File,
    start: Instant,
    sequence: u64,
}
thread_local! { static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) }; }
pub(super) fn start(uuid: u128, name: &str) -> io::Result<()> {
    if std::env::var_os("HORIZON_COMMAND_SCRIPT").is_none() {
        return Ok(());
    }
    let Some(path) = std::env::var_os("HORIZON_COMMAND_PACKET_TRACE") else {
        return Ok(());
    };
    let mut file = File::create(path)?;
    writeln!(
        file,
        "{}",
        json!({"event":"start","connection":0,"uuid":format!("{uuid:032x}"),"name":name,
        "unix_ns":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos().to_string()})
    )?;
    TRACE.with(|t| {
        *t.borrow_mut() = Some(Trace {
            file,
            start: Instant::now(),
            sequence: 0,
        })
    });
    Ok(())
}
pub(super) fn packet(id: i32, payload: &[u8]) -> io::Result<()> {
    TRACE.with(|t| {
        let mut t=t.borrow_mut(); let Some(t)=t.as_mut() else { return Ok(()); };
        let data:String=payload.iter().map(|b|format!("{b:02x}")).collect();
        writeln!(t.file,"{}",json!({"event":"packet","connection":0,"sequence":t.sequence,
            "elapsed_ns":t.start.elapsed().as_nanos().to_string(),"state":"play","dir":"s2c","id":id,
            "data":data,"size":payload.len()}))?;
        t.sequence+=1; t.file.flush()
    })
}
pub(super) fn finish() -> io::Result<()> {
    TRACE.with(|t| {
        if let Some(mut t) = t.borrow_mut().take() {
            writeln!(
                t.file,
                "{}",
                json!({"event":"end","connection":0,"packets":t.sequence})
            )?;
            t.file.flush()?;
        }
        Ok(())
    })
}
