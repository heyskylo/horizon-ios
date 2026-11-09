//! Original silent Vorbis fixture and barrier controls for delayed file reads.
//! No output device, game assets, or user home is opened.
use super::*;
use base64::Engine;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn scratch(label: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "horizon-audio-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn silence(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(SILENCE)
        .unwrap();
    // Read the original Vorbis identification packet independently of the
    // decoder: this fixture has always been 8 kHz stereo, not mono.
    assert_eq!(&bytes[..4], b"OggS");
    let packet = 27 + usize::from(bytes[26]);
    assert_eq!(&bytes[packet..packet + 7], b"\x01vorbis");
    assert_eq!(bytes[packet + 11], 2);
    assert_eq!(
        u32::from_le_bytes(bytes[packet + 12..packet + 16].try_into().unwrap()),
        8000
    );
    fs::write(path, bytes).unwrap();
}

fn wait_owner(owner: &Arc<()>) {
    let end = Instant::now() + Duration::from_secs(5);
    while !poll_retired_for(Some(owner)) {
        assert!(Instant::now() < end, "audio reader did not retire");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(unix)]
#[test]
fn delayed_decode_keeps_both_overlay_and_source_after_reload() {
    let home = scratch("overlay");
    let root = home.join("cache/mod-content/0123456789abcdef");
    let source = home.join("cache/mod-sources").join("a".repeat(64));
    let file = root.join("assets/test/sounds/late.ogg");
    silence(&source.join("late.ogg"));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(source.join("late.ogg"), &file).unwrap();
    let snapshot = RootLease::combine([
        RootLease::acquire(&root).unwrap(),
        RootLease::acquire(&source).unwrap(),
    ]);
    snapshot.register_alias(&root);
    let mut audio = Audio::silent(&root);
    let owner = audio.backend.work_owner.clone();
    let old_cache = audio.backend.cache.clone();
    audio.backend.live = true;
    let (send, receive) = mpsc::channel();
    audio.backend.decoder = Some(send);
    let channel = audio.backend.create(false).unwrap();
    let sound = Sound {
        location: "test:late".into(),
        volume: crate::sound_events::Sampled::Constant(1.0),
        pitch: crate::sound_events::Sampled::Constant(1.0),
        weight: 1,
        event: false,
        stream: false,
        preload: false,
        attenuation_distance: 16,
        sentinel: Default::default(),
    };
    // Use the real production queue path to construct the delayed Decode payload.
    audio.backend.start(channel, &sound, false);
    let job = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let (entered, ready) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    let (result, decoded) = mpsc::channel();
    audio.backend.decoder_work = Some(AudioWork::spawn(
        "held decoder",
        owner.clone(),
        move |cancel| {
            assert!(!cancelled(&cancel));
            // Model preemption after the cancellation check, before decode opens its file.
            entered.send(()).unwrap();
            proceed.recv_timeout(Duration::from_secs(5)).unwrap();
            let (samples, rate, channels) =
                job.load().expect("the old linked source is still readable");
            result.send((samples.len(), rate, channels)).unwrap();
        },
    ));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    audio.reload_with(&home.join("replacement"), SoundRegistry::default());
    old_cache.lock().unwrap().insert(
        "test:sounds/late.ogg".into(),
        Some((Arc::new(vec![1.0]), 8000, 1)),
    );
    assert!(
        audio
            .backend
            .cache
            .lock()
            .unwrap()
            .get("test:sounds/late.ogg")
            .is_none(),
        "a retired decoder cannot fill the new generation's cache"
    );
    drop(snapshot);
    assert!(
        !audio.shutdown_workers(),
        "the already-started read is still owned"
    );
    // A different player's departure never waits for this blocked decoder.
    let mut other = audio.share(1);
    assert!(other.shutdown_workers());
    drop(audio);
    assert!(RootLease::try_exclusive(&root).unwrap().is_none());
    assert!(RootLease::try_exclusive(&source).unwrap().is_none());
    release.send(()).unwrap();
    let (samples, rate, channels) = decoded.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(samples > 0);
    assert_eq!(rate, 8000);
    assert_eq!(channels, 2, "preserve the fixture header’s stereo channels");
    assert_eq!(
        samples % usize::from(channels),
        0,
        "complete interleaved frames"
    );
    wait_owner(&owner);
    assert!(RootLease::try_exclusive(&root).unwrap().is_some());
    assert!(RootLease::try_exclusive(&source).unwrap().is_some());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn stream_read_from_prepared_base_finishes_before_the_retirement_gate_opens() {
    let home = scratch("base");
    let file = home.join("assets/test/sounds/late.ogg");
    silence(&file);
    let mut audio = Audio::silent(&home);
    let owner = audio.backend.work_owner.clone();
    let (entered, ready) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    let (result, decoded) = mpsc::channel();
    let read_file = file.clone();
    audio.backend.streams.push(AudioWork::spawn(
        "held stream",
        owner.clone(),
        move |cancel| {
            assert!(!cancelled(&cancel));
            entered.send(()).unwrap();
            proceed.recv_timeout(Duration::from_secs(5)).unwrap();
            let (samples, rate, channels) =
                decode(&read_file).expect("base is not replaced until this reader retires");
            result.send((samples.len(), rate, channels)).unwrap();
        },
    ));
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!audio.shutdown_workers());
    assert!(
        !audio.backend.live,
        "no new decode can start while shutting down"
    );
    drop(audio);
    assert!(
        !poll_retired_for(Some(&owner)),
        "dropping Audio must not detach its stream reader"
    );
    release.send(()).unwrap();
    let (samples, rate, channels) = decoded.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(samples > 0);
    assert_eq!(rate, 8000);
    assert_eq!(channels, 2, "preserve the fixture header’s stereo channels");
    assert_eq!(
        samples % usize::from(channels),
        0,
        "complete interleaved frames"
    );
    wait_owner(&owner);
    // This is the boundary at which the app may give an importer the prepared base.
    fs::write(&file, b"replacement generation").unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"replacement generation");
    fs::remove_dir_all(home).unwrap();
}

// 30 ms of generated silence, 8 kHz stereo Vorbis, produced with FFmpeg's
// native encoder (no third-party game/audio source). Filled below as base64.
const SILENCE: &str = "T2dnUwACAAAAAAAAAACxq+IWAAAAABKJNxgBHgF2b3JiaXMAAAAAAkAfAAAAAAAAAAAAAAAAAAC7AU9nZ1MAAAAAAAAAAAAAsaviFgEAAADduo3bDj3///////////////+7A3ZvcmJpcw0AAABMYXZmNjIuMTIuMTAxAQAAABwAAABlbmNvZGVyPUxhdmM2Mi4yOC4xMDEgdm9yYmlzAQV2b3JiaXMcQkNWAgAQAACEdJpZqgEizECGgdCQlQAAAgAAYIQiDDEgNGQlAAAQAAAghpKDaEJrzjfnOGiWg6ZSbE4HJ1JtnuSmYm7OOeecc7I5Z4xzzjmnKGcWg2ZCa845JzFoloJmQmvOOedJbB60pkprzjlnnHM6GGeEcc45p0lrHqRmY23OOWdBa5qj5lJszjknUm6e1OZSbc4555xzzjnnnHPOOad6cToH54Rzzjknam+u5SZ0cc4555NxujcnhHPOOeecc84555xzzjknCA1ZCQAAAQAQhGFjGHcKgvQ5GohRhJiGTHrQPTpMgsYgp5B6NDoaKaUOQkllnJTSCUJDVgIAgAAAEEJIIYUUUkghhRRSSCGFGGKIIYaccsopqKCSSiqqKKPMMssss8wyyyyzDjvrrMMOQwwxxNBKK7HUVFuNNdaae8655iCtldZaa62UUkoppZSC0JCVAAAIAACBkEEGGWQUUkghhRhiyimnnIIKKiA0ZCUAAAYAwCFnoIEGGmiggQYaaKBxxhmIIIIIIqikkkw6Cim12GrMMddegw46955777n4HIRSSimllFJKKaWUUkopJQgNWQkAgAAAAAghhBBSSCGFFFKKMcYccw46CSUEQkNWAgBgAAAMMcQYZJBBSCGFGGKKKccccww6CCGUUlJooYVcaoglllZaiaWlmGqLsdZYc+0x1t5777333nvvvffee86B0JCVAEAEAACDDCKIIIKMMQYhBISGrAQAQAAAEGKIMcYghBBSiCGnnIJMMumko5ACoSErAQAnAACEEUckcQQSZ6CBCCqpIKPMQiyxtdZaa6211lprrbXWWmuttdZaa6211lprrbXWWmstEBqyEgCIAABgkEEGGUQQQQQZZIDQkJUAAAgAACOMQAQZpRRjjjnmGHTQQScdhRZaIDRkJQDgBABAIKGIMswwBBFVVFFGFVUUUkcppZRSSimllFJKKaWUUkoppZRSSimllFJKKaVUSimlBEJDVgIAZAAAkKKUUiktRYIipRikGEtGFXNQWoqocgxSzalSziDmJJaIMYSUk1Qy5hRCDELqHHVMKQYtlRhCxhik2HJLoXMOCA1ZIQCEZgA4HAeQLAuQLAsAAAAAAAAAJE0DNM8DLM0DAAAAAAAAAEnTAMvTAM3zAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACQNA3QPA/QPA8AAAAAAAAANM8DPE8EPFEEAAAAAAAAAMvzAE30AE8UAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACQNA3QPA/QPA8AAAAAAAAALM8DPFEENE8EAAAAAAAAAMvzAE8UAU/0AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAACjgAAApYCIWGrAgA4gQAHJIESYIkQfMAkmVB06BpME2AZFnQNGgaTBMAAAAAAAAAAAAASdOgadA0iCJA0jRoGjQNoggAAAAAAAAAAACApGnQNGgaRBEgaRo0DZoGUQQAAAAAAAAAAADAM02IIkQRpgnwTBOiCFGEaQIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAACRwAAAVMKAOFhqwIAOIEAByOYlkAAOA4jmUBAIDjOJYFAACWZYkiAABYliaKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAJHAAABUwoA4WGrAQAogAAHIpiWcBxLAs4jmUBSbIsgGUBNA+gaQBRBAACAAANHAAABWzQlFgcoNCQlQBAFACAQXEsS9NEkSRpmuaJIknSNM8TRZrmeZ5nmvA8zzNNiKIomiZEURRNE6ZpmqoKTFNVBQAAGjgAAArYoCmxOEChISsBgJAAAIeiWJameZ7niaJpqiZJ0jTPE0VRNE3TVFWSpGmeJ4qiaJqmqaosS9M8TxRF0TRVVVWhaZ4niqJomqqquvA8zxNFUTRNVXVdeJ7niaIomqaqui5EURRN0zRVU1VdF4iiaZqmqqqq6wLRE0XTVFXXdV3geaJomqrqqq4LRNM0VVVVXVeWAaZpmqrqurIMUFVVdV3XlWWAqqqq67quLANU1XVdV5ZlGYDruq4sy7IAAEAEBwBAASPoJKPKImw04cIDUGjIigAgCgAAMIYpxZQyjEkIKYSGMQkhhZBJSam0lCoIqZRUSgUhlZJKySillFpKFYRUSiqlgpBKSaUUAACL4AAAi2AhFBqyEgDIAwAgjFGKMcackwgpxZhzzkmElGLMOeekUow555xzUkrGHHPOOSmlc84555yUkjnnnHNOSumcc845J6WU0jnnnJNSSgmhc9BJKaV0zjnnBACAGjgAAArYKLI5wUhQoSErAYBUAACD41iWpnmeKJqmJUma5nmeJ4qmqUmSpnme54miavI8zxNFUTRNVeV5nieKomiaqsp1RdE0TVNVVZcsi6Jpmqaqui5M0zRV1XVdF6Zpmqrquq4L21ZVVXVdWYZtq6qquq4sA9d1XVm2ZSDLriu7tiwAAF7BAQDUwIbVEU6KxgILDVkJAGQAABDGIKQQQkgZhJBCCCGlFEICAAAJHAAABUwoA4WGrAQAUgEAAGOstdZaa6010FlrrbXWWisgs9Zaa6211lprrbXWWmuttdRaa6211lprrbXWWmuttdZaa6211lprrbXWWmuttdZaa6211lprrbXWWmuttdZaa6211lpLKaWUUkoppZRSSimllFJKKaWUUgFAvxYOAP8QNqyOcFI0FlhoyEoAIBwAADBGKcYcg1BKKRVCjDknHZXWYqwQYsw5CSm1FlvxnHMQSkiltRiL55yDUEpKsdVYVAqhlJRSiy3WolLoqKSUUms1FmNMKqm11mKrsRhjUgottdZijMUIW1NqLbbaaizG2JpKCy3GGGMxwhcZW4uptlqDMcLIFktLtdYajDFG99Ziqa3mYowPvrYUS4w1FwDg7uAAgKhg4wwrSWeFo8GFhqwEAEICAAiElGKMMcacc845qRRjjjnnnIMQQiiVYowx55yDEEIIJWOMOecchBBCCKGUkjHnHIQQQgghpJQ65xyEEEIIIYRSSuecgxBCCCGEUErpIIQQQgghhBJKKSmFEEIIIYQQQioppRBCCKGUEEpIJaUUQgghhFBKCSmllEIIoZQQQighpZRSSiGEEEIppaSUUkqphFJCCaGEVEpKKYUSQgillJJSSimVUkIooYRSSkkppZRSCCGEUkoBAIAIDgCAAkbQSUaVRdhowoUHIAAAAAQAIAiRGSJRsAAMDlQAQsIUAFBYYJADAA0OD2kXF9BlgAu6uOtACEEIQhCLAyggAQcn3PDEG55wgxN0ikoNCAAAAAAAGQB8AAAkD0BERDRzEBESExQVFhcYGRobHAAAgAACAAAQAAAAAAAIAAAAABBPZ2dTAAQAAQAAAAAAALGr4hYCAAAAsSx1iwIeHh4AAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB4AAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
