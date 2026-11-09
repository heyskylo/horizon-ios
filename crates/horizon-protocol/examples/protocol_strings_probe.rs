//! The packet string parity corpus, without a connection or game resources.
use horizon_protocol::buf::Reader;
use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mut output = BufWriter::new(File::create(&args[2])?);
    for line in BufReader::new(File::open(&args[1])?).lines() {
        let line = line?;
        let (limit, hex) = line.split_once(' ').ok_or("missing string fixture")?;
        if hex.len() % 2 != 0 {
            return Err("odd hex length".into());
        }
        let bytes = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair)?, 16).map_err(Box::<dyn Error>::from)
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        let mut reader = Reader::new(&bytes);
        match reader.string(limit.parse()?) {
            Ok(value) => {
                write!(output, "OK {} ", reader.position())?;
                for unit in value.encode_utf16() {
                    write!(output, "{unit:04x}")?;
                }
                writeln!(output)?;
            }
            Err(_) => writeln!(output, "ERR {}", reader.position())?,
        }
    }
    Ok(())
}
