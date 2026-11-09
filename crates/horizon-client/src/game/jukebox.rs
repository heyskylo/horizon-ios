//! Jukeboxes (`JukeboxBlock`, `JukeboxBlockEntity`, `JukeboxSongPlayer`):
//! inserting a `jukebox_playable` disc, ejecting it, the song with its
//! "Now Playing" overlay, note particles every second, the redstone
//! signal while playing, the comparator output, and saving.
use super::Game;
use horizon_client::{
    block_entity_item,
    containers::{BlockEntity, Jukebox},
    first_person::Hand,
    items::ItemStack,
    mobs::Species,
    particles::{ParticleOptions, ParticleType},
    text::Text,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::level::flags as level_flags;

/// The stack's `jukebox_playable` component (`JukeboxPlayable`), else the
/// item's default one: `None` when it has none (a removed component,
/// `!minecraft:jukebox_playable`, included), `Some(None)` for an inline song
/// definition, which has no id in the song registry.
fn jukebox_playable(
    record: &ItemStack,
    items: &horizon_client::items::Items,
) -> Option<Option<String>> {
    let data = record.data();
    if data
        .and_then(|d| d.other("!minecraft:jukebox_playable"))
        .is_some()
    {
        return None;
    }
    match data.and_then(|d| d.other("minecraft:jukebox_playable")) {
        Some(horizon_world::nbt::Tag::String(song)) => Some(Some(song.to_string_lossy())),
        Some(_) => Some(None),
        None => Some(Some(items.get(&record.item)?.jukebox_song.clone()?)),
    }
}

/// `JukeboxSong.fromStack`: the song the disc names, by its id.
fn song_of_record(record: &ItemStack, items: &horizon_client::items::Items) -> Option<String> {
    jukebox_playable(record, items).flatten()
}

impl Game<'_> {
    fn jukebox(&self, pos: [i32; 3]) -> Option<&Jukebox> {
        match self.world.block_entities.get(&(pos[0], pos[1], pos[2])) {
            Some(BlockEntity::Jukebox(j)) => Some(j),
            _ => None,
        }
    }
    fn set_has_record(&mut self, pos: [i32; 3], has: bool) {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let registry = self.pack.assets.registry;
        let Some(mut s) = registry.state(state).cloned() else {
            return;
        };
        s.properties.insert(
            "has_record".into(),
            if has { "true" } else { "false" }.into(),
        );
        if let Some(new) = registry.id(&s) {
            let p = BlockPos::new(pos[0], pos[1], pos[2]);
            self.with_level(|b, level, ticks| {
                b.set_block(level, ticks, p, new, level_flags::UPDATE_CLIENTS)
            });
        }
    }
    /// `JukeboxBlock.setPlacedBy`: a jukebox placed from an item whose
    /// `block_entity_data` carries a `RecordItem` keeps that record, so
    /// `has_record` turns on: the model draws the disc and `useWithoutItem`
    /// pops it out. Vanilla reads the item's data, after
    /// `updateCustomBlockEntityTag` has filled the block entity.
    pub(super) fn jukebox_placed(&mut self, pos: [i32; 3], stack: &ItemStack) {
        if block_entity_item::carries_record(stack) {
            self.set_has_record(pos, true);
        }
    }
    /// `onSongChanged`: neighbours (the signal) and comparators.
    fn jukebox_changed(&mut self, pos: [i32; 3]) {
        let p = BlockPos::new(pos[0], pos[1], pos[2]);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.with_level(|b, level, ticks| {
            let block = b.block(state);
            b.update_neighbors_at(level, ticks, p, block);
        });
        self.container_changed((pos[0], pos[1], pos[2]));
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
    }
    /// `JukeboxBlockEntity.setTheItem` from a container (a hopper or a dropper
    /// feeding the slot): a disc that lands in a jukebox that is not playing
    /// starts its song, as the player's own use does, with the block state,
    /// the song's level event and the neighbour and comparator updates.
    pub(super) fn jukebox_inserted(&mut self, pos: [i32; 3]) {
        let Some(song) = self
            .jukebox(pos)
            .filter(|j| !j.playing)
            .and_then(|j| j.record())
            .and_then(|record| song_of_record(record, self.items()))
        else {
            return;
        };
        if let Some(BlockEntity::Jukebox(j)) =
            self.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
        {
            j.playing = true;
            j.song_clock_present = true;
            j.ticks = 0;
        }
        self.set_has_record(pos, true);
        self.start_song(pos, &song);
        self.jukebox_changed(pos);
    }
    /// `JukeboxBlockEntity.setTheItem(EMPTY)` from a container (a hopper
    /// below taking the disc): the song stops, the block loses its record,
    /// and the neighbours and comparators hear it.
    pub(super) fn jukebox_taken(&mut self, pos: [i32; 3]) {
        let mut was_playing = false;
        if let Some(BlockEntity::Jukebox(j)) =
            self.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
        {
            if j.record().is_some() {
                return;
            }
            was_playing = j.playing;
            j.playing = false;
            j.song_clock_present = false;
            j.ticks = 0;
        }
        // `JukeboxSongPlayer.stop` posts only while a song plays.
        if was_playing {
            self.stop_song_heard(pos);
        } else {
            self.stop_song(pos);
        }
        self.set_has_record(pos, false);
        self.jukebox_changed(pos);
    }
    pub(super) fn jukebox_set_item(&mut self, pos: [i32; 3], stack: Option<ItemStack>) {
        let song = stack.as_ref().and_then(|s| song_of_record(s, self.items()));
        let has_record = stack.is_some();
        let Some(BlockEntity::Jukebox(j)) =
            self.world.block_entities.get_mut(&(pos[0], pos[1], pos[2]))
        else {
            return;
        };
        let was_playing = j.playing;
        j.items[0] = stack;
        j.playing = song.is_some();
        j.song_clock_present = song.is_some();
        j.ticks = 0;
        self.set_has_record(pos, has_record);
        match song {
            Some(song) => self.start_song(pos, &song),
            None if was_playing => self.stop_song_heard(pos),
            None => {}
        }
        self.jukebox_changed(pos);
    }
    /// `JukeboxBlock.useItemOn` / `useWithoutItem` on the jukebox at `pos`.
    pub(super) fn use_jukebox(&mut self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        if &*self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]))
            .name
            != "minecraft:jukebox"
        {
            return false;
        }
        if self.jukebox(pos).is_some_and(|j| j.record().is_some()) {
            // TRY_WITH_EMPTY_HAND: only the main hand goes on to useWithoutItem.
            if self.hand != Hand::Main {
                return false;
            }
            self.eject_record(pos);
            // ServerPlayerGameMode.useItemOn: useWithoutItem consumed the click.
            self.trigger_default_block_use(pos);
            return true;
        }
        let Some(stack) = held else { return false };
        // JukeboxBlock.useItemOn: an item without JUKEBOX_PLAYABLE tries the empty hand.
        let Some(song) = jukebox_playable(stack, self.res.items()) else {
            return false;
        };
        // JukeboxPlayable.tryInsertIntoJukebox
        let record = ItemStack {
            count: 1,
            ..stack.clone()
        };
        if !self.creative {
            self.consume_held();
        }
        self.award_custom_stat("play_record", 1);
        self.world.block_entities.insert(
            (pos[0], pos[1], pos[2]),
            BlockEntity::Jukebox(Jukebox {
                items: [Some(record)],
                ticks: 0,
                playing: true,
                song_clock_present: true,
            }),
        );
        self.set_has_record(pos, true);
        if let Some(song) = song {
            self.start_song(pos, &song);
        }
        self.jukebox_changed(pos);
        // ServerPlayerGameMode.useItemOn: the action was consumed.
        self.trigger_block("minecraft:item_used_on_block", pos, stack);
        true
    }
    /// `JukeboxBlock.useItemOn` on this client's level for the main hand:
    /// whether the jukebox at `pos` takes the click (`has_record` pops the
    /// disc out, or a `jukebox_playable` item goes in). The server does the
    /// rest, see `use_jukebox`.
    pub(super) fn jukebox_takes(&self, pos: [i32; 3], held: Option<&ItemStack>) -> bool {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if &*self.res.facts().get(state).name != "minecraft:jukebox" {
            return false;
        }
        let has_record = self
            .pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("has_record"))
            .is_some_and(|v| v == "true");
        has_record || held.is_some_and(|stack| jukebox_playable(stack, self.res.items()).is_some())
    }
    /// Level event 1010: the song from the jukebox, and the "Now Playing" overlay.
    fn start_song(&mut self, pos: [i32; 3], song: &str) {
        self.stop_sound(pos);
        let Some(s) = self.items().songs.get(song).cloned() else {
            return;
        };
        // LevelRenderer.playJukeboxSong: SimpleSoundInstance.forJukeboxSong.
        let id = self.audio.play_jukebox_song(&s.sound, pos);
        self.jukebox_sounds.insert((pos[0], pos[1], pos[2]), id);
        let description =
            horizon_client::component::to_text(&s.description, &mut |k| Text::literal(k));
        // Hud.setNowPlaying: the overlay, said at once.
        let message = Text::translate("record.nowPlaying", &[description]);
        self.say_system_now(&message);
        self.titles.set_overlay(message, true);
        self.update_song_volumes();
        self.notify_record_playing(pos, true);
        if self.audio.capture.is_some() {
            self.lan_songs.push((pos, Some(song.to_owned())));
        }
    }
    /// `LevelEventHandler` 1010 on a client of a server: `playJukeboxSong`
    /// for the song the event's data names in the server's `jukebox_song`
    /// registry (`Registry.get(int).ifPresent`: an unknown id does nothing).
    /// The server's jukebox plays it; this client only hears it.
    pub(super) fn jukebox_song_event(&mut self, pos: [i32; 3], data: i32) {
        if let Some(song) = jukebox_song_name(self.remote.as_ref().map(|r| &*r.registries), data) {
            self.start_song(pos, &song);
        }
    }
    /// `LevelEventHandler` 1011: `stopJukeboxSongAndNotifyNearby`.
    pub(super) fn jukebox_stop_event(&mut self, pos: [i32; 3]) {
        self.stop_sound(pos);
        self.notify_record_playing(pos, false);
    }
    fn stop_sound(&mut self, pos: [i32; 3]) {
        if let Some(id) = self.jukebox_sounds.remove(&(pos[0], pos[1], pos[2])) {
            self.audio.stop_and_release(id);
        }
    }
    /// Level event 1011: the song ends, here and for the guests.
    fn stop_song(&mut self, pos: [i32; 3]) {
        self.stop_sound(pos);
        self.notify_record_playing(pos, false);
        if self.audio.capture.is_some() {
            self.lan_songs.push((pos, None));
        }
    }
    /// `JukeboxSongPlayer.stop` of a jukebox whose record left it, taken out by
    /// hand, pulled by a hopper or finished: the song stops and the
    /// `JUKEBOX_STOP_PLAY` vibration goes to the listeners around it. A broken
    /// jukebox is not this path: its removal posts the same event from the
    /// world's spill, so it must not post it here as well.
    fn stop_song_heard(&mut self, pos: [i32; 3]) {
        self.stop_song(pos);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.post_block_game_event("minecraft:jukebox_stop_play", pos, None, Some(state));
    }

    /// `LevelEventHandler.notifyNearbyEntities` (level events 1010 and 1011):
    /// parrots within three blocks of the jukebox start or stop dancing.
    pub(super) fn notify_record_playing(&mut self, pos: [i32; 3], playing: bool) {
        let min = Vec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
        let area = Aabb::new(min, min.add_vector(Vec3::new(1.0, 1.0, 1.0)))
            .inflate(Vec3::new(3.0, 3.0, 3.0));
        for m in self
            .mobs
            .iter_mut()
            .filter(|m| m.species() == Species::Parrot && m.bounds().intersects(area))
        {
            m.extra.party_jukebox = playing.then_some(pos);
        }
    }
    /// `popOutTheItem`: the disc above the jukebox, and the song stops.
    fn eject_record(&mut self, pos: [i32; 3]) {
        let key = (pos[0], pos[1], pos[2]);
        let mut was_playing = false;
        let record = match self.world.block_entities.get_mut(&key) {
            Some(BlockEntity::Jukebox(j)) => {
                was_playing = j.playing;
                j.playing = false;
                j.song_clock_present = false;
                j.ticks = 0;
                j.items[0].take()
            }
            _ => None,
        };
        // `JukeboxSongPlayer.stop` posts only while a song plays: a disc
        // taken out after its song ended is silent.
        if was_playing {
            self.stop_song_heard(pos);
        } else {
            self.stop_song(pos);
        }
        if let Some(record) = record {
            // atLowerCornerWithOffset(pos, 0.5, 1.01, 0.5).offsetRandomXZ(random, 0.7);
            // `new ItemEntity` moves it with its own random.
            let at = horizon_world::level::removal::jukebox_pop_position(
                &mut self.world.random,
                horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]),
            );
            self.spawn_item_own(record, at, 10);
        }
        self.set_has_record(pos, false);
        self.jukebox_changed(pos);
    }
    /// A broken jukebox stops its song (its disc drops with the contents).
    pub(super) fn jukebox_removed(&mut self, pos: [i32; 3]) {
        self.stop_song(pos);
    }
    /// `JukeboxSongPlayer.tick` of the jukebox at `key`, while it plays.
    pub(super) fn tick_jukebox(&mut self, key: (i32, i32, i32)) {
        let playing = match self.world.block_entities.get(&key) {
            Some(BlockEntity::Jukebox(j)) if j.playing => j.record().and_then(|r| {
                let song = song_of_record(r, self.items())?;
                Some((song, j.ticks))
            }),
            _ => None,
        };
        let Some((song, ticks)) = playing else {
            return;
        };
        let pos = [key.0, key.1, key.2];
        let Some(s) = self.items().songs.get(&song).cloned() else {
            return;
        };
        if s.finished(ticks) {
            if let Some(BlockEntity::Jukebox(j)) = self.world.block_entities.get_mut(&key) {
                j.playing = false;
                j.song_clock_present = false;
                j.ticks = 0;
            }
            self.stop_song_heard(pos);
            self.jukebox_changed(pos);
            return;
        }
        if !self.jukebox_sounds.contains_key(&key) && ticks == 0 {
            self.start_song(pos, &song);
        }
        if ticks % 20 == 0 {
            // `JukeboxSongPlayer.tick`: `gameEvent(JUKEBOX_PLAY, pos,
            // Context.of(state))` every twenty ticks while a song plays, for
            // the sculk listeners and the allays around it.
            let state = self.world.block(pos[0], pos[1], pos[2]);
            self.post_block_game_event("minecraft:jukebox_play", pos, None, Some(state));
            let color = f64::from(self.world.random.next_int_bound(4).unwrap_or(0)) / 24.0;
            let at = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + f64::from(1.2_f32), // add(0.0, 1.2F, 0.0)
                f64::from(pos[2]) + 0.5,
            ];
            self.particles.add(
                &ParticleOptions::simple(ParticleType::Note),
                at,
                [color, 0.0, 0.0],
            );
        }
        if let Some(BlockEntity::Jukebox(j)) = self.world.block_entities.get_mut(&key) {
            j.ticks += 1;
        }
    }
    /// Let go of songs that ended or whose jukebox stopped.
    pub(super) fn update_song_volumes(&mut self) {
        let sounds: Vec<_> = self
            .jukebox_sounds
            .iter()
            .map(|(k, id)| (*k, *id))
            .collect();
        for (k, id) in sounds {
            // A server's client has no playing jukebox of its own: its songs
            // stop with level event 1011, or when they end.
            let playing = matches!(self.world.block_entities.get(&k), Some(BlockEntity::Jukebox(j)) if j.playing);
            if !keeps_song(
                self.audio.engine.is_active(id),
                self.remote.is_some(),
                playing,
            ) {
                self.audio.stop_and_release(id);
                self.jukebox_sounds.remove(&k);
            }
        }
    }
}
/// The song a level event 1010's data names: its id in the `jukebox_song`
/// registry the server sent (`Registry.get(int)`). Only a server's client
/// has one; a level that plays its own jukeboxes starts their songs itself.
fn jukebox_song_name(
    registries: Option<&horizon_protocol::registry::Dynamic>,
    id: i32,
) -> Option<String> {
    registries?
        .name("minecraft:jukebox_song", id)
        .map(str::to_owned)
}
/// Whether a jukebox song keeps playing: until it ends, and, in a level that
/// plays its own jukeboxes, while that jukebox plays. A server's client
/// stops it at level event 1011 (`LevelRenderer.playingJukeboxSongs`).
fn keeps_song(active: bool, server_client: bool, jukebox_playing: bool) -> bool {
    active && (server_client || jukebox_playing)
}
/// The jukebox's redstone signal (15 while a song plays) and comparator output.
pub(super) fn jukebox_signals(
    world: &horizon_client::world::World,
    items: &horizon_client::items::Items,
    pos: BlockPos,
) -> Option<(i32, i32)> {
    match world.block_entities.get(&(pos.x, pos.y, pos.z)) {
        Some(BlockEntity::Jukebox(j)) => {
            let comparator = j
                .record()
                .and_then(|r| song_of_record(r, items))
                .and_then(|s| items.songs.get(&s).map(|song| song.comparator))
                .unwrap_or(0);
            Some((if j.playing { 15 } else { 0 }, comparator))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{jukebox_song_name, keeps_song};
    use horizon_protocol::registry::Dynamic;

    /// On a server's client the song comes from level event 1010, whose data
    /// is the song's id in the server's `jukebox_song` registry (#3173), and
    /// it plays on although this client's jukebox isn't playing.
    #[test]
    fn a_server_clients_jukebox_song_comes_from_level_event_1010() {
        let mut registries = Dynamic::default();
        registries.registries.insert(
            "minecraft:jukebox_song".into(),
            ["minecraft:11", "minecraft:13", "minecraft:cat"]
                .map(|n| (n.to_owned(), None))
                .to_vec(),
        );
        assert_eq!(
            jukebox_song_name(Some(&registries), 2).as_deref(),
            Some("minecraft:cat")
        );
        assert_eq!(jukebox_song_name(Some(&registries), 3), None);
        assert_eq!(jukebox_song_name(Some(&registries), -1), None);
        assert_eq!(jukebox_song_name(None, 2), None);
        // A server's client: on until the song ends (or level event 1011).
        assert!(keeps_song(true, true, false));
        assert!(!keeps_song(false, true, false));
        // A level that plays its own jukebox: also until the jukebox stops.
        assert!(keeps_song(true, false, true));
        assert!(!keeps_song(true, false, false));
    }
}

#[cfg(test)]
mod jukebox_play_tests {
    use super::*;

    /// A windowless game from the prepared resources with a jukebox at
    /// (0, 64, 0), a disc in hand and its song started, ticked `ticks` times:
    /// the game events it posted, in order. `None` without the resources.
    fn playing(ticks: i32) -> Option<Vec<String>> {
        use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES")?);
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .ok()?;
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        // `World::set` writes into a resident chunk only.
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        let jukebox = game
            .pack
            .assets
            .registry
            .default_state("minecraft:jukebox")?;
        game.world.set(0, 64, 0, jukebox).unwrap();
        let disc = ItemStack::new("minecraft:music_disc_cat", 1);
        crate::game::dispense_fixture::start_recording();
        assert!(
            game.use_jukebox([0, 64, 0], Some(&disc)),
            "the disc goes in"
        );
        for _ in 0..ticks {
            game.tick_jukebox((0, 64, 0));
        }
        Some(crate::game::dispense_fixture::stop_recording())
    }

    /// `JukeboxSongPlayer.tick` posts `JUKEBOX_PLAY` at the jukebox every
    /// `PLAY_EVENT_INTERVAL_TICKS` (20) ticks of a song, next to the note
    /// particle it already drew. Horizon drew the particle and kept the
    /// listeners deaf: a sculk sensor beside a playing jukebox never clicked.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_playing_jukebox_posts_jukebox_play_every_twenty_ticks() {
        let Some(lines) = playing(41) else { return };
        let plays: Vec<&String> = lines
            .iter()
            .filter(|l| l.ends_with("minecraft:jukebox_play"))
            .collect();
        assert_eq!(
            plays,
            vec![
                &"G 0,64,0 minecraft:jukebox_play".to_owned(),
                &"G 0,64,0 minecraft:jukebox_play".to_owned(),
                &"G 0,64,0 minecraft:jukebox_play".to_owned(),
            ],
            "{lines:?}"
        );
        // The song is still playing after 41 ticks.
        assert!(
            lines.iter().all(|l| l.ends_with("minecraft:jukebox_play")),
            "{lines:?}"
        );
    }

    /// A jukebox whose song is stopped posts no play event: only the twenty
    /// ticks of a playing song do.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_stopped_jukebox_posts_no_play_event() {
        let Some(lines) = playing(0) else { return };
        assert!(lines.is_empty(), "{lines:?}");
    }
}

#[cfg(test)]
mod saved_song_clock_tests {
    use crate::game::native_machines::tests::server_game;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and scratch homes/cwd"]
    fn actual_start_tick_stop_and_loaded_record_use_the_modeled_clock() {
        let mut game = server_game();
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("forceload add 0 0 15 15"));
        assert!(game.world.loaded(4, 4));
        assert!(game.run_command("setblock 4 199 4 stone"));
        assert!(game.run_command("setblock 4 200 4 jukebox"));
        let clock = game.world.game_time;
        let mut failures = Vec::new();
        let mut check = |label: &str, tag: &horizon_world::nbt::Tag, want: Option<i64>| {
            let got = tag.get("ticks_since_song_started").cloned();
            let want = want.map(horizon_world::nbt::Tag::Long);
            if got != want {
                failures.push(format!("{label}: {got:?}, expected {want:?}"));
            }
        };
        let pos = [4, 200, 4];
        check(
            "actual empty Game writer",
            &game.block_entity_nbt(pos).unwrap(),
            None,
        );
        assert!(game.run_command("item replace block 4 200 4 container.0 with music_disc_cat[minecraft:custom_data={h032:1}]"));
        let j = game.jukebox(pos).unwrap();
        assert!(j.playing);
        assert_eq!(j.ticks, 0);
        assert_eq!(j.record().unwrap().count, 1);
        check(
            "actual start keeps zero Long",
            &game.block_entity_nbt(pos).unwrap(),
            Some(0),
        );
        game.tick_jukebox((4, 200, 4));
        assert_eq!(game.jukebox(pos).unwrap().ticks, 1);
        check(
            "actual tick keeps sole counter",
            &game.block_entity_nbt(pos).unwrap(),
            Some(1),
        );
        let before = game
            .block_entity_nbt(pos)
            .unwrap()
            .get("RecordItem")
            .cloned();
        assert!(before.is_some());
        game.jukebox_set_item(pos, None);
        let j = game.jukebox(pos).unwrap();
        assert!(!j.playing);
        assert_eq!(j.ticks, 0);
        assert!(j.record().is_none());
        let saved = game.block_entity_nbt(pos).unwrap();
        assert!(saved.get("RecordItem").is_none());
        check("actual stop removes clock", &saved, None);
        assert!(game.run_command("setblock 8 199 4 stone"));
        assert!(game.run_command("setblock 8 200 4 jukebox[has_record=true]{RecordItem:{id:\"minecraft:music_disc_cat\",count:1,components:{\"minecraft:custom_data\":{h032:1}}}}"));
        let j = game.jukebox([8, 200, 4]).unwrap();
        assert!(j.playing, "preserve legacy record-only adoption");
        assert_eq!(j.ticks, 0);
        let saved = game.block_entity_nbt([8, 200, 4]).unwrap();
        assert_eq!(
            saved.get("RecordItem"),
            before.as_ref(),
            "actual record/components preserved"
        );
        check("actual loaded record without clock", &saved, None);
        assert_eq!(
            game.world.game_time, clock,
            "no scene ticks in this caller test; registered runtime progress is separately paired"
        );
        assert!(
            failures.is_empty(),
            "actual Game saved song-clock mismatches: {failures:?}"
        );
    }
}

#[cfg(test)]
mod jukebox_component_tests {
    use super::super::native_machines::tests::server_game;
    use super::*;
    use horizon_core::position::BlockPos;
    use horizon_world::nbt::Tag;

    fn comparator(game: &mut super::Game<'_>, stack: ItemStack) -> i32 {
        let key = (8, 64, 8);
        game.world.block_entities.insert(
            key,
            BlockEntity::Jukebox(Jukebox {
                items: [Some(stack)],
                ticks: 0,
                song_clock_present: true,
                playing: true,
            }),
        );
        jukebox_signals(
            &game.world,
            game.items(),
            BlockPos::new(key.0, key.1, key.2),
        )
        .unwrap()
        .1
    }

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_jukebox_reads_the_discs_own_jukebox_component() {
        let mut game = server_game();
        let cat = game.items().songs.get("minecraft:cat").unwrap().comparator;
        assert_eq!(
            comparator(&mut game, ItemStack::new("minecraft:music_disc_cat", 1)),
            cat,
            "the item's default song"
        );

        let mut removed = ItemStack::new("minecraft:music_disc_cat", 1);
        removed.data_mut().set_other(
            "!minecraft:jukebox_playable",
            Some(Tag::Compound(Default::default())),
        );
        assert_eq!(
            comparator(&mut game, removed),
            0,
            "a removed playable component names no song"
        );

        let mut other = ItemStack::new("minecraft:stone", 1);
        other.data_mut().set_other(
            "minecraft:jukebox_playable",
            Some(Tag::String("minecraft:cat".into())),
        );
        assert_eq!(
            comparator(&mut game, other),
            cat,
            "the stack's own component names the song"
        );
    }

    /// `JukeboxBlock.useItemOn` asks the stack for `JUKEBOX_PLAYABLE`: a disc
    /// whose component was removed is not taken, any item given one is.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_jukebox_takes_what_the_stacks_component_makes_playable() {
        let mut game = server_game();
        let state = game
            .pack
            .assets
            .registry
            .default_state("minecraft:jukebox")
            .unwrap();
        game.world.set(8, 64, 8, state).unwrap();
        let takes =
            |game: &super::Game<'_>, stack: ItemStack| game.jukebox_takes([8, 64, 8], Some(&stack));
        assert!(takes(&game, ItemStack::new("minecraft:music_disc_cat", 1)));
        assert!(!takes(&game, ItemStack::new("minecraft:stone", 1)));
        let mut removed = ItemStack::new("minecraft:music_disc_cat", 1);
        removed.data_mut().set_other(
            "!minecraft:jukebox_playable",
            Some(Tag::Compound(Default::default())),
        );
        assert!(
            !takes(&game, removed),
            "a removed component is not playable"
        );
        let mut stone = ItemStack::new("minecraft:stone", 1);
        stone.data_mut().set_other(
            "minecraft:jukebox_playable",
            Some(Tag::String("minecraft:cat".into())),
        );
        assert!(
            takes(&game, stone),
            "the stack's own component makes it playable"
        );
    }
}
