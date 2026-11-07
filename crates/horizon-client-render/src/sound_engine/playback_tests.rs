use super::*;
use horizon_core::random::RandomSource;

fn setup() -> (SoundEngine, test_backend::Null) {
    let mut registry = SoundRegistry::default();
    registry.add_file(
        "test",
        br#"{"beep":{"subtitle":"test.beep","sounds":[{"name":"beep","volume":0.5,"pitch":0.5}]}}"#,
        &|_, _| true,
    );
    let mut engine = SoundEngine::new(registry);
    engine.playback_hook = None;
    engine.listening = true;
    (engine, test_backend::Null::default())
}
fn instance() -> SoundInstance {
    SoundInstance::at(
        "test:beep",
        4,
        0.8,
        1.2,
        LegacyRandom::new(1),
        [2.0, 3.0, 4.0],
    )
}
fn adjust(sound: &SoundPlayback<'_>, volume: &mut f32, pitch: &mut f32, _: &mut bool) {
    assert_eq!(sound.identifier, "test:beep");
    assert_eq!(sound.source, 4);
    assert!(!sound.relative);
    *volume *= 0.5;
    *pitch = 2.0;
}

#[test]
fn played_parameters_reach_channels_without_changing_the_instance() {
    let (mut engine, mut backend) = setup();
    engine.set_test_playback_hook(adjust);
    let (id, result) = engine.play_new(instance(), true, &mut backend);
    assert_eq!(result, PlayResult::Started);
    let channel = engine.channel(id).unwrap();
    assert_eq!(backend.volume[&channel], 0.2);
    assert_eq!(backend.pitch[&channel], 1.0);
    assert_eq!(backend.position[&channel], [2.0, 3.0, 4.0]);
    assert_eq!(engine.instances[&id].volume, 0.8);
    assert_eq!(engine.instances[&id].pitch, 1.2);
    assert_eq!(engine.notifications.len(), 1);
}

#[test]
fn delayed_and_manually_looped_plays_do_not_compound_edits() {
    let (mut engine, mut backend) = setup();
    engine.set_test_playback_hook(adjust);
    let mut sound = instance();
    sound.looping = true;
    sound.delay = 2;
    let id = engine.add(sound, true);
    engine.play_delayed(id, 2);
    engine.tick(false, &TickContext::default(), &mut backend);
    assert!(backend.volume.is_empty() && engine.notifications.is_empty());
    engine.tick(false, &TickContext::default(), &mut backend);
    let first = engine.channel(id).unwrap();
    assert_eq!(backend.volume[&first], 0.2);
    backend.stopped.insert(first, true);
    for _ in 0..22 {
        engine.tick(false, &TickContext::default(), &mut backend);
    }
    let replay = engine.channel(id).unwrap();
    assert_ne!(first, replay);
    assert_eq!(backend.volume[&replay], 0.2);
    assert_eq!(engine.instances[&id].volume, 0.8);
}

#[test]
fn tickable_and_category_updates_keep_transient_edits_and_can_cancel() {
    let (mut engine, mut backend) = setup();
    engine.set_test_playback_hook(adjust);
    let mut sound = instance();
    sound.ticking = Ticking::Native;
    let (id, _) = engine.play_new(sound, true, &mut backend);
    let channel = engine.channel(id).unwrap();
    let sound = engine.instance_mut(id).unwrap();
    sound.volume = 0.4;
    sound.position = [8.0, 9.0, 10.0];
    engine.tick(false, &TickContext::default(), &mut backend);
    assert_eq!(backend.volume[&channel], 0.1);
    assert_eq!(backend.position[&channel], [8.0, 9.0, 10.0]);
    engine.set_test_playback_hook(|_, volume, _, _| *volume *= 0.5);
    engine.update_category_volume(4, 0.5, &mut backend);
    assert_eq!(backend.volume[&channel], 0.05);
    assert_eq!(
        backend.pitch[&channel], 0.6,
        "a later no-pitch edit restores the base pitch"
    );
    engine.notifications.clear();
    engine.set_test_playback_hook(|sound, _, _, cancel| {
        assert!(!sound.starting);
        *cancel = true;
    });
    engine.tick(false, &TickContext::default(), &mut backend);
    assert!(backend.stopped(channel));
    assert!(engine.notifications.is_empty());
}

#[test]
fn cancellation_and_invalid_edits_never_allocate_or_notify() {
    let edits: [PlaybackHook; 6] = [
        |_, _, _, cancel| *cancel = true,
        |_, volume, _, _| *volume = f32::NAN,
        |_, volume, _, _| *volume = f32::INFINITY,
        |_, volume, _, _| *volume = -1.0,
        |_, _, pitch, _| *pitch = 0.0,
        |_, _, pitch, _| *pitch = f32::NEG_INFINITY,
    ];
    for hook in edits {
        let (mut engine, mut backend) = setup();
        engine.set_test_playback_hook(hook);
        let (_, result) = engine.play_new(instance(), false, &mut backend);
        assert_eq!(result, PlayResult::NotStarted);
        assert!(backend.stopped.is_empty() && engine.notifications.is_empty());
    }
}

#[test]
fn no_op_dispatch_preserves_random_sampling_and_failed_pool_behavior() {
    let (mut plain, mut plain_backend) = setup();
    let (mut hooked, mut hooked_backend) = setup();
    hooked.set_test_playback_hook(|_, _, _, _| {});
    for (engine, backend) in [
        (&mut plain, &mut plain_backend),
        (&mut hooked, &mut hooked_backend),
    ] {
        engine.limits = [0, 0];
        let id = engine.add(instance(), true);
        assert_eq!(engine.play(id, backend), PlayResult::NotStarted);
        engine.limits = [247, 8];
        assert_eq!(engine.play(id, backend), PlayResult::Started);
    }
    assert_eq!(plain_backend.volume, hooked_backend.volume);
    assert_eq!(plain_backend.pitch, hooked_backend.pitch);
    assert_eq!(plain.notifications, hooked.notifications);
    assert_eq!(
        plain.instances.get_mut(&1).unwrap().random.next_long(),
        hooked.instances.get_mut(&1).unwrap().random.next_long()
    );
}

#[test]
fn entity_bound_updates_are_intercepted_at_the_new_position() {
    let (mut engine, mut backend) = setup();
    let entity = SoundEntity::new(42, [2.0, 3.0, 4.0], false);
    engine.set_test_playback_hook(|sound, volume, _, _| {
        if !sound.starting {
            assert_eq!(sound.position, [8.0, 9.0, 10.0]);
        }
        *volume *= 0.5;
    });
    let sound = SoundInstance::entity_bound(
        "test:beep",
        4,
        0.8,
        1.0,
        LegacyRandom::new(1),
        entity.clone(),
    );
    let (id, _) = engine.play_new(sound, true, &mut backend);
    entity.update(SoundEntityState {
        position: [8.0, 9.0, 10.0],
        silent: false,
        removed: false,
    });
    engine.tick(false, &TickContext::default(), &mut backend);
    let channel = engine.channel(id).unwrap();
    assert_eq!(backend.position[&channel], [8.0, 9.0, 10.0]);
    assert_eq!(backend.volume[&channel], 0.2);
}

#[test]
fn a_volume_only_edit_preserves_legacy_zero_pitch_clamping() {
    let (mut engine, mut backend) = setup();
    engine.set_test_playback_hook(|_, volume, _, _| *volume *= 0.5);
    let mut sound = instance();
    sound.pitch = 0.0;
    let (id, result) = engine.play_new(sound, true, &mut backend);
    assert_eq!(result, PlayResult::Started);
    let channel = engine.channel(id).unwrap();
    assert_eq!(backend.volume[&channel], 0.2);
    assert_eq!(backend.pitch[&channel], 0.5);
}
