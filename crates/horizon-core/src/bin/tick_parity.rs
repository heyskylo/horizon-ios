//! Original trace adapter for pinned vanilla scheduler comparisons.
use horizon_core::{
    position::{BlockPos, ChunkPos},
    ticks::{
        ChunkTicks, LevelTicks, SavedTick, ScheduleResult, ScheduledTick, TickArea, TickPriority,
        TickTypeId,
    },
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    io::{self, BufRead, Write},
    str::{FromStr, SplitWhitespace},
};
struct Input<'a>(SplitWhitespace<'a>);
impl Input<'_> {
    fn value<T: FromStr>(&mut self) -> Result<T, Box<dyn Error>>
    where
        T::Err: Error + 'static,
    {
        Ok(self.0.next().ok_or("missing argument")?.parse()?)
    }
    fn pos(&mut self) -> Result<BlockPos, Box<dyn Error>> {
        Ok(BlockPos::new(self.value()?, self.value()?, self.value()?))
    }
    fn chunk(&mut self) -> Result<ChunkPos, Box<dyn Error>> {
        Ok(ChunkPos::new(self.value()?, self.value()?))
    }
    fn area(&mut self) -> Result<TickArea, Box<dyn Error>> {
        Ok(TickArea::new(self.pos()?, self.pos()?))
    }
}
fn result(result: ScheduleResult) -> u8 {
    match result {
        ScheduleResult::Added => 1,
        ScheduleResult::Duplicate => 0,
        ScheduleResult::MissingChunk => 2,
    }
}
fn scheduled(out: &mut impl Write, tick: ScheduledTick) -> io::Result<()> {
    writeln!(
        out,
        "queued {} {} {} {} {} {} {}",
        tick.kind.0,
        tick.pos.x,
        tick.pos.y,
        tick.pos.z,
        tick.trigger_tick,
        tick.priority as i8,
        tick.sub_tick_order
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut level = LevelTicks::default();
    let mut detached = HashMap::<ChunkPos, ChunkTicks>::new();
    let mut blocked = HashSet::new();
    let mut out = io::BufWriter::new(io::stdout().lock());
    for (line_number, line) in io::stdin().lock().lines().enumerate() {
        let line = line?;
        let mut input = Input(line.split_whitespace());
        let operation = input.0.next().ok_or("empty command")?;
        writeln!(out, "command {} {}", line_number, operation)?;
        match operation {
            "reset" => {
                level = LevelTicks::default();
                detached.clear();
                blocked.clear();
            }
            "add" => {
                let pos = input.chunk()?;
                if level.chunk(pos).is_none() {
                    level.add_chunk(pos, ChunkTicks::default())?;
                }
            }
            "load" => {
                let pos = input.chunk()?;
                let count: usize = input.value()?;
                let mut ticks = Vec::with_capacity(count);
                for _ in 0..count {
                    ticks.push(SavedTick {
                        kind: TickTypeId(input.value()?),
                        pos: input.pos()?,
                        delay: input.value()?,
                        priority: TickPriority::by_value(input.value()?),
                    });
                }
                if level.chunk(pos).is_some() {
                    level.remove_chunk(pos);
                }
                level.add_chunk(pos, ChunkTicks::from_saved(ticks))?;
            }
            "unpack" => {
                let pos = input.chunk()?;
                level.unpack_chunk(pos, input.value()?);
            }
            "remove" => {
                let pos = input.chunk()?;
                if let Some(chunk) = level.remove_chunk(pos) {
                    detached.insert(pos, chunk);
                }
            }
            "restore" => {
                let pos = input.chunk()?;
                if level.chunk(pos).is_none()
                    && let Some(chunk) = detached.remove(&pos)
                {
                    level.add_chunk(pos, chunk)?;
                }
            }
            "enable" => {
                let pos = input.chunk()?;
                if input.value::<i32>()? == 0 {
                    blocked.insert(pos);
                } else {
                    blocked.remove(&pos);
                }
            }
            "schedule" => {
                let tick = ScheduledTick {
                    kind: TickTypeId(input.value()?),
                    pos: input.pos()?,
                    trigger_tick: input.value()?,
                    priority: TickPriority::by_value(input.value()?),
                    sub_tick_order: input.value()?,
                };
                writeln!(out, "schedule {}", result(level.schedule(tick)))?;
            }
            "query" => {
                let kind = TickTypeId(input.value()?);
                let pos = input.pos()?;
                writeln!(
                    out,
                    "query {} {}",
                    u8::from(level.has_scheduled(pos, kind)),
                    u8::from(level.will_tick_this_tick(pos, kind))
                )?;
            }
            "chunk" => {
                let pos = input.chunk()?;
                if let Some(chunk) = level.chunk(pos) {
                    writeln!(out, "chunk {}", chunk.count())?;
                    for &tick in chunk.queued() {
                        scheduled(&mut out, tick)?;
                    }
                } else {
                    writeln!(out, "chunk absent")?;
                }
            }
            "save" => {
                let pos = input.chunk()?;
                let time = input.value()?;
                if let Some(chunk) = level.chunk(pos) {
                    let ticks = chunk.pack(time);
                    writeln!(out, "saved {}", ticks.len())?;
                    for tick in ticks {
                        writeln!(
                            out,
                            "saved-value {} {} {} {} {} {}",
                            tick.kind.0,
                            tick.pos.x,
                            tick.pos.y,
                            tick.pos.z,
                            tick.delay,
                            tick.priority as i8
                        )?;
                    }
                } else {
                    writeln!(out, "saved absent")?;
                }
            }
            "reload" => {
                let pos = input.chunk()?;
                let save_time = input.value()?;
                let load_time = input.value()?;
                if let Some(old) = level.remove_chunk(pos) {
                    let mut chunk = ChunkTicks::from_saved(old.pack(save_time));
                    chunk.unpack(load_time);
                    level.add_chunk(pos, chunk)?;
                }
            }
            "clear" => level.clear_area(input.area()?),
            "copy" => {
                let area = input.area()?;
                level.copy_area(area, input.pos()?);
            }
            "tick" => {
                let time: i64 = input.value()?;
                let maximum: i32 = input.value()?;
                let action: i32 = input.value()?;
                let probe_kind = TickTypeId(input.value()?);
                let probe = input.pos()?;
                let mut order: i64 = input.value()?;
                level.begin_tick(time, maximum, |pos| !blocked.contains(&pos))?;
                while let Some(tick) = level.next_tick()? {
                    scheduled(&mut out, tick)?;
                    writeln!(
                        out,
                        "running {} {} {}",
                        u8::from(level.will_tick_this_tick(tick.pos, tick.kind)),
                        u8::from(level.has_scheduled(tick.pos, tick.kind)),
                        u8::from(level.will_tick_this_tick(probe, probe_kind))
                    )?;
                    match action {
                        1 => {
                            let mut next = tick;
                            next.trigger_tick = time;
                            next.sub_tick_order = order;
                            order = order.wrapping_add(1);
                            writeln!(out, "callback-schedule {}", result(level.schedule(next)))?;
                        }
                        2 => {
                            level.clear_area(TickArea::new(tick.pos, tick.pos.offset(1, 1, 1)));
                            writeln!(
                                out,
                                "callback-clear {}",
                                u8::from(level.will_tick_this_tick(probe, probe_kind))
                            )?;
                        }
                        3 => level.copy_area(
                            TickArea::new(tick.pos.offset(-2, -2, -2), tick.pos.offset(2, 2, 2)),
                            BlockPos::new(16, 0, 0),
                        ),
                        4 => {
                            let pos = tick.pos.chunk();
                            if let Some(chunk) = level.remove_chunk(pos) {
                                detached.insert(pos, chunk);
                            }
                        }
                        5 => {
                            blocked.insert(tick.pos.chunk());
                        }
                        0 => {}
                        _ => return Err("unknown callback action".into()),
                    }
                }
                level.finish_tick()?;
            }
            _ => return Err(format!("unknown command {operation}").into()),
        }
        if input.0.next().is_some() {
            return Err("extra arguments".into());
        }
        writeln!(out, "count {}", level.count())?;
    }
    Ok(())
}
