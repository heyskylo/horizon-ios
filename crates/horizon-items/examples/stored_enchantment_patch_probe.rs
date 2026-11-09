//! Saved component patches; input is label, command argument and the equivalent stack SNBT.
use horizon_items::item_data::{canon_stack, parse_stack};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
};

fn main() -> horizon_items::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let mut output = BufWriter::new(File::create(&args[2])?);
    for line in BufReader::new(File::open(&args[1])?).lines() {
        let line = line?;
        let fields: Vec<_> = line.splitn(3, '\t').collect();
        if fields.len() != 3 {
            return Err("expected label, command argument and saved stack".into());
        }
        let stack = parse_stack(fields[2])?.ok_or("expected a nonempty stack")?;
        writeln!(output, "{}\t{}", fields[0], canon_stack(Some(&stack)))?;
    }
    Ok(())
}
