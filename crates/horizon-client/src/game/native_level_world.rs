//! A level's world settings for native server mods, through a `LevelRef`
//! (docs/native-mods.md, "Entity and level handles", #7387): the seed, the
//! difficulty, game rules and weather. They return `None` on a client or for
//! a handle of an earlier open.
use super::Game;
use horizon_client::command::vanilla::DIFFICULTIES;
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
