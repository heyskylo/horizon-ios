//! A level's world settings for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): the seed, the
//! difficulty, game rules and weather, and the setters of its time, game rules
//! and weather. The readers return `None` on a client or for a handle of an
//! earlier open; the setters return false.
use super::Game;
use horizon_client::command::vanilla::DIFFICULTIES;
use horizon_client::gamerules;
use horizon_client::mod_api::{GameRuleValue, LevelRef};

/// `Level.isRaining` and `isThundering` from the rain and thunder levels.
fn weather_flags((rain, thunder): (f32, f32)) -> (bool, bool) {
    (rain > 0.2, thunder > 0.9)
}

impl Game<'_> {
    /// The world's seed (`WorldOptions.seed`), as `/seed` shows it. API
    /// @API:level_world@.
    pub(super) fn native_level_seed(&self, level: LevelRef) -> Option<i64> {
        self.native_level_world(level)?;
        Some(self.level.seed)
    }
    /// The difficulty (`Level.getDifficulty`): `peaceful`, `easy`, `normal`
    /// or `hard`, as `/difficulty` names it. API @API:level_world@.
    pub(super) fn native_level_difficulty(&self, level: LevelRef) -> Option<&'static str> {
        self.native_level_world(level)?;
        DIFFICULTIES.get(usize::from(self.ui.difficulty)).copied()
    }
    /// Game rule `rule` (`minecraft:keep_inventory` or `keep_inventory`), as
    /// `/gamerule` queries it: None for an unknown rule. API @API:level_world@.
    pub(super) fn native_level_game_rule(
        &self,
        level: LevelRef,
        rule: &str,
    ) -> Option<GameRuleValue> {
        self.native_level_world(level)?;
        let r = horizon_client::gamerules::rule(rule)?;
        self.ui.rules.get(r.name)
    }
    /// The rain and thunder levels, each 0.0 to 1.0 (`Level.getRainLevel`,
    /// `getThunderLevel`): both 0 in a level with no weather, as the Nether
    /// and the End. API @API:level_world@.
    pub(super) fn native_level_weather(&self, level: LevelRef) -> Option<(f32, f32)> {
        let world = self.native_level_world(level)?;
        if level.dimension() != super::Dim::Overworld.id() {
            return Some((0.0, 0.0));
        }
        Some(world.weather.levels(1.0))
    }
    /// Whether it rains (`Level.isRaining`: a rain level above 0.2). API
    /// @API:level_world@.
    pub(super) fn native_level_is_raining(&self, level: LevelRef) -> Option<bool> {
        Some(weather_flags(self.native_level_weather(level)?).0)
    }
    /// Whether it thunders (`Level.isThundering`: a thunder level above 0.9).
    /// API @API:level_world@.
    pub(super) fn native_level_is_thundering(&self, level: LevelRef) -> Option<bool> {
        Some(weather_flags(self.native_level_weather(level)?).1)
    }
    /// Sets the time of day, as `/time set <ticks>` does on the overworld clock: mods hear
    /// `time_skip` (cause `mod`) first and can keep the old time, and the clock's fraction of a
    /// tick restarts. False for a negative time, for a handle that isn't this open's on the
    /// server, or when a mod kept the old time; a time the clock already shows is true and
    /// changes nothing. API @API:level_world_setters@.
    pub(super) fn native_level_set_day_time(&mut self, level: LevelRef, time: i64) -> bool {
        if time < 0 || self.native_level_world(level).is_none() {
            return false;
        }
        let clock = "minecraft:overworld";
        let total = self.command_clock_time(clock);
        total == time || self.jump_clock(clock, total, time, "mod")
    }
    /// Sets game rule `rule` (`minecraft:advance_time` or `advance_time`) as `/gamerule` does,
    /// so `advance_time` and `advance_weather` freeze and unfreeze the day clock and the weather
    /// cycle. False for an unknown rule, a value of the wrong kind or out of the rule's range,
    /// or a handle that isn't this open's on the server. API @API:level_world_setters@.
    pub(super) fn native_level_set_game_rule(
        &mut self,
        level: LevelRef,
        rule: &str,
        value: GameRuleValue,
    ) -> bool {
        let Some(rule) = gamerules::rule(rule) else {
            return false;
        };
        if self.native_level_world(level).is_none() || !self.ui.rules.clone().set(rule.name, value)
        {
            return false;
        }
        self.set_game_rule(rule, value);
        true
    }
    /// Sets the weather as `/weather` does (`ServerLevel.setWeatherParameters`): `clear_time`
    /// ticks of clear weather, then `weather_time` ticks of the new state (rain, thunder or
    /// neither) as `raining` and `thundering` say. Mods hear `weather_change` and
    /// `thunder_change` first and can keep the old state. False for a negative time, a level that
    /// can't have weather (the Nether, the End) or isn't the one in place, a handle that isn't
    /// this open's on the server, or when a mod kept the old state. API
    /// @API:level_world_setters@.
    pub(super) fn native_level_set_weather(
        &mut self,
        level: LevelRef,
        clear_time: i32,
        weather_time: i32,
        raining: bool,
        thundering: bool,
    ) -> bool {
        if clear_time < 0
            || weather_time < 0
            || level.dimension() != self.dim.id()
            || self.native_level_world(level).is_none()
            || !self.can_have_weather()
        {
            return false;
        }
        self.apply_weather(clear_time, weather_time, raining, thundering)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rain_and_thunder_start_above_vanillas_thresholds() {
        assert_eq!(super::weather_flags((0.2, 0.0)), (false, false));
        assert_eq!(super::weather_flags((0.21, 0.9)), (true, false));
        assert_eq!(super::weather_flags((1.0, 0.95)), (true, true));
    }
}
