//! Binary transport for exhaustive comparison of the compiled float log sampler.
use horizon_core::density::{DensitySampler, SampleError, SamplerContext, UnaryOp, UnarySampler};
use std::{
    io::{self, Write},
    sync::Arc,
};

#[derive(Debug)]
struct FloatBitsInput;
impl DensitySampler for FloatBitsInput {
    fn sample_value(&self, _: &mut SamplerContext, position: [i32; 3]) -> Result<f32, SampleError> {
        Ok(f32::from_bits(position[0] as u32))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut start = args
        .next()
        .ok_or("missing first float bits")?
        .parse::<u64>()?;
    let mut remaining = args.next().ok_or("missing float count")?.parse::<u64>()?;
    if args.next().is_some()
        || start
            .checked_add(remaining)
            .is_none_or(|end| end > 1_u64 << 32)
    {
        return Err("invalid float bit range".into());
    }
    let sampler = UnarySampler {
        input: Arc::new(FloatBitsInput),
        operation: UnaryOp::Log,
    };
    let mut context = SamplerContext::default();
    let mut out = io::BufWriter::with_capacity(1 << 20, io::stdout().lock());
    let mut buffer = vec![0_u8; 1 << 20];
    while remaining > 0 {
        let entries = remaining.min((buffer.len() / 4) as u64) as usize;
        for (offset, bytes) in buffer[..entries * 4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let bits = (start + offset as u64) as u32;
            let value = sampler.sample_value(&mut context, [bits as i32, 0, 0])?;
            let result = if value.is_nan() {
                0x7fc0_0000
            } else {
                value.to_bits()
            };
            bytes.copy_from_slice(&result.to_be_bytes());
        }
        out.write_all(&buffer[..entries * 4])?;
        start += entries as u64;
        remaining -= entries as u64;
    }
    out.flush()?;
    Ok(())
}
