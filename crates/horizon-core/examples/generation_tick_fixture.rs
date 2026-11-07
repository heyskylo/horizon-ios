//! Private native differential transport for generation-stage tick containers.
use horizon_core::{
    position::BlockPos,
    ticks::{
        ChunkTicks, ProtoChunkTicks, SavedTick, ScheduledTick, TickContainerAccess, TickPriority,
        TickTypeId, WorldGenTickAccess, WorldGenTickContainers,
    },
};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    str::{FromStr, SplitWhitespace},
};
enum Container {
    Proto(ProtoChunkTicks),
    Level(ChunkTicks),
}
impl TickContainerAccess for Container {
    fn schedule(&mut self, t: ScheduledTick) -> bool {
        match self {
            Self::Proto(c) => c.schedule(t),
            Self::Level(c) => c.schedule(t),
        }
    }
    fn has_scheduled(&self, p: BlockPos, k: TickTypeId) -> bool {
        match self {
            Self::Proto(c) => c.has_scheduled(p, k),
            Self::Level(c) => c.has_scheduled(p, k),
        }
    }
}
#[derive(Default)]
struct World {
    chunks: BTreeMap<(i32, i32), Container>,
    events: Vec<String>,
    fail: bool,
}
impl WorldGenTickContainers for World {
    type Container = Container;
    type Error = ();
    fn container_at(&mut self, p: BlockPos) -> Result<&mut Container, ()> {
        self.events.push(format!("lookup {} {} {}", p.x, p.y, p.z));
        if self.fail {
            return Err(());
        }
        Ok(self
            .chunks
            .entry((p.x >> 4, p.z >> 4))
            .or_insert_with(|| Container::Proto(ProtoChunkTicks::default())))
    }
}
fn number<T: FromStr>(input: &mut SplitWhitespace<'_>) -> T
where
    T::Err: std::fmt::Debug,
{
    input.next().unwrap().parse().unwrap()
}
fn pos(input: &mut SplitWhitespace<'_>) -> BlockPos {
    BlockPos::new(number(input), number(input), number(input))
}
fn saved(input: &mut SplitWhitespace<'_>) -> SavedTick {
    SavedTick {
        kind: TickTypeId(number(input)),
        pos: pos(input),
        delay: number(input),
        priority: TickPriority::by_value(number(input)),
    }
}
fn emit(out: &mut impl Write, ticks: &[SavedTick]) -> io::Result<()> {
    writeln!(out, "saved {}", ticks.len())?;
    for t in ticks {
        writeln!(
            out,
            "value {} {} {} {} {} {}",
            t.kind.0, t.pos.x, t.pos.y, t.pos.z, t.delay, t.priority as i8
        )?;
    }
    Ok(())
}
fn flush(access: &mut WorldGenTickAccess<World>, out: &mut impl Write) -> io::Result<()> {
    for e in access.containers_mut().events.drain(..) {
        writeln!(out, "{e}")?;
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut access = WorldGenTickAccess::new(World::default());
    let mut snapshot = Vec::new();
    let mut out = io::BufWriter::new(io::stdout().lock());
    for (index, line) in io::stdin().lock().lines().enumerate() {
        let line = line?;
        let mut input = line.split_whitespace();
        let op = input.next().unwrap();
        writeln!(out, "command {index} {op}")?;
        match op {
            "reset" => {
                access = WorldGenTickAccess::new(World::default());
                snapshot.clear();
            }
            "fail" => access.containers_mut().fail = number::<i32>(&mut input) != 0,
            "load" | "level" => {
                let key = (number(&mut input), number(&mut input));
                let count: usize = number(&mut input);
                let ticks = (0..count).map(|_| saved(&mut input)).collect::<Vec<_>>();
                let c = if op == "load" {
                    Container::Proto(ProtoChunkTicks::from_saved(ticks))
                } else {
                    Container::Level(ChunkTicks::from_saved(ticks))
                };
                access.containers_mut().chunks.insert(key, c);
            }
            "schedule" => {
                let t = ScheduledTick {
                    kind: TickTypeId(number(&mut input)),
                    pos: pos(&mut input),
                    trigger_tick: number(&mut input),
                    priority: TickPriority::by_value(number(&mut input)),
                    sub_tick_order: number(&mut input),
                };
                let result = access.schedule(t);
                flush(&mut access, &mut out)?;
                writeln!(
                    out,
                    "{}",
                    if result.is_ok() { "scheduled" } else { "error" }
                )?;
            }
            "query" => {
                let k = TickTypeId(number(&mut input));
                let p = pos(&mut input);
                let result = access.has_scheduled(p, k);
                flush(&mut access, &mut out)?;
                match result {
                    Ok(v) => writeln!(out, "query {v}")?,
                    Err(()) => writeln!(out, "error")?,
                }
            }
            "will" => {
                let k = TickTypeId(number(&mut input));
                let p = pos(&mut input);
                writeln!(out, "will {}", access.will_tick_this_tick(p, k))?;
            }
            "dump" => {
                let key = (number(&mut input), number(&mut input));
                let time = number(&mut input);
                match access.containers().chunks.get(&key) {
                    Some(Container::Proto(c)) => emit(&mut out, c.pack(time))?,
                    Some(Container::Level(c)) => emit(&mut out, &c.pack(time))?,
                    None => writeln!(out, "absent")?,
                }
            }
            "snapshot" => {
                let key = (number(&mut input), number(&mut input));
                if let Some(Container::Proto(c)) = access.containers().chunks.get(&key) {
                    snapshot = c.scheduled_ticks();
                } else {
                    snapshot.clear();
                }
            }
            "snapshot_dump" => emit(&mut out, &snapshot)?,
            "unpack" | "promote" => {
                let key = (number(&mut input), number(&mut input));
                let time = number(&mut input);
                if let Some(c) = access.containers_mut().chunks.get_mut(&key) {
                    if op == "promote"
                        && let Container::Proto(proto) = c
                    {
                        *c = Container::Level(ChunkTicks::from_saved(proto.scheduled_ticks()));
                    }
                    if let Container::Level(c) = c {
                        c.unpack(time);
                    }
                }
            }
            "remove" => {
                access
                    .containers_mut()
                    .chunks
                    .remove(&(number(&mut input), number(&mut input)));
            }
            "count" => {}
            _ => panic!("unknown operation {op}"),
        }
        assert!(input.next().is_none());
        writeln!(out, "count {}", access.count())?;
    }
    Ok(())
}
