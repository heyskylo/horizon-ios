//! What a server's diagnostics mod reads (docs/native-mods.md, "Server hooks
//! for plugin ports: diagnostics"): the tick clock as vanilla and Bukkit
//! report it. Reads only; nothing here changes what the server does.
use super::Game;

// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// The last 100 server ticks' durations in nanoseconds, as vanilla's
    /// `MinecraftServer.getTickTimesNanos` lays them out: the tick numbered
    /// `n` at index `n % 100` (0 where none was recorded yet). Sort a copy
    /// for percentiles; the average is `native_tick_stats`'. On a client, its
    /// own ticks. API @API:tick_times@.
    pub(super) fn native_tick_times(&self) -> Vec<u64> {
        self.clock.tick_times_nanos()
    }
    /// The ticks a second over the last one, five and fifteen minutes
    /// (Bukkit's `Server.getTPS`), measured between the ends of the recorded
    /// ticks and averaged once a second as a load average is; 20 at the
    /// start. A sprint reads more than the rate, a server that falls behind
    /// less. API @API:tick_times@.
    pub(super) fn native_tps(&self) -> [f64; 3] {
        self.clock.tps()
    }
}
