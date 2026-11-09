//! Shared by the audio fixtures: a backend recording what an OpenAL source
//! would hold, and the per-step dump the Java oracles print.
use horizon_client::sound_engine::{Backend, ChannelId, InstanceId, PlayResult, SoundEngine};
use horizon_client::sound_events::Sound;
use std::collections::HashMap;
use std::io::Write;

const INITIAL: u32 = 0x1011;
const PLAYING: u32 = 0x1012;
const PAUSED: u32 = 0x1013;
const STOPPED: u32 = 0x1014;

/// An OpenAL source's state as `alGetSource*` reports it.
#[derive(Clone)]
pub struct Source {
    pub state: u32,
    pub gain: f32,
    pub pitch: f32,
    pub position: [f32; 3],
    pub relative: bool,
    pub looping: bool,
    pub model: u32,
    pub max: f32,
    pub rolloff: f32,
    pub reference: f32,
}
impl Default for Source {
    fn default() -> Self {
        Self {
            state: INITIAL,
            gain: 1.0,
            pitch: 1.0,
            position: [0.0; 3],
            relative: false,
            looping: false,
            model: 0xD002,
            max: f32::MAX,
            rolloff: 1.0,
            reference: 1.0,
        }
    }
}
/// Buffers and streams are ready at once (silence in the oracle); stopping an
/// initial source leaves it initial, as in OpenAL Soft.
#[derive(Default)]
pub struct Fake {
    pub sources: HashMap<ChannelId, Source>,
    next: ChannelId,
}
impl Backend for Fake {
    fn create(&mut self, _streaming: bool) -> Option<ChannelId> {
        self.next += 1;
        self.sources.insert(self.next, Source::default());
        Some(self.next)
    }
    fn destroy(&mut self, channel: ChannelId) {
        self.sources.remove(&channel);
    }
    fn set_pitch(&mut self, c: ChannelId, pitch: f32) {
        self.sources.get_mut(&c).expect("source").pitch = pitch;
    }
    fn set_volume(&mut self, c: ChannelId, volume: f32) {
        self.sources.get_mut(&c).expect("source").gain = volume;
    }
    fn set_attenuation(&mut self, c: ChannelId, linear: Option<f32>) {
        let s = self.sources.get_mut(&c).expect("source");
        match linear {
            Some(max) => {
                s.model = 0xD003;
                s.max = max;
                s.rolloff = 1.0;
                s.reference = 0.0;
            }
            None => s.model = 0,
        }
    }
    fn set_looping(&mut self, c: ChannelId, looping: bool) {
        self.sources.get_mut(&c).expect("source").looping = looping;
    }
    fn set_position(&mut self, c: ChannelId, p: [f64; 3]) {
        self.sources.get_mut(&c).expect("source").position =
            [p[0] as f32, p[1] as f32, p[2] as f32];
    }
    fn set_relative(&mut self, c: ChannelId, relative: bool) {
        self.sources.get_mut(&c).expect("source").relative = relative;
    }
    fn start(&mut self, c: ChannelId, _sound: &Sound, _looping: bool) {
        self.sources.get_mut(&c).expect("source").state = PLAYING;
    }
    fn stop(&mut self, c: ChannelId) {
        if let Some(s) = self.sources.get_mut(&c)
            && s.state != INITIAL
        {
            s.state = STOPPED;
        }
    }
    fn pause(&mut self, c: ChannelId) {
        let s = self.sources.get_mut(&c).expect("source");
        if s.state == PLAYING {
            s.state = PAUSED;
        }
    }
    fn unpause(&mut self, c: ChannelId) {
        let s = self.sources.get_mut(&c).expect("source");
        if s.state == PAUSED {
            s.state = PLAYING;
        }
    }
    fn stopped(&mut self, c: ChannelId) -> bool {
        self.sources.get(&c).is_none_or(|s| s.state == STOPPED)
    }
}
pub fn bits(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}
pub fn f(hex: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(hex, 16).expect("float"))
}
pub fn d(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("double"))
}
/// The shared per-step dump (instances, notifications, pool counts).
pub fn describe(engine: &SoundEngine, backend: &Fake, name: &str, id: InstanceId) -> String {
    let mut s = format!("  {name} active={}", engine.is_active(id));
    match engine.channel(id).and_then(|c| backend.sources.get(&c)) {
        None => s.push_str(" channel=-"),
        Some(src) => s.push_str(&format!(
            " state={:x} gain={} pitch={} pos={},{},{} rel={} loop={} model={:x} max={} rolloff={} ref={}",
            src.state,
            bits(src.gain),
            bits(src.pitch),
            bits(src.position[0]),
            bits(src.position[1]),
            bits(src.position[2]),
            u8::from(src.relative),
            u8::from(src.looping),
            src.model,
            bits(src.max),
            bits(src.rolloff),
            bits(src.reference)
        )),
    }
    s
}
pub fn flush(engine: &mut SoundEngine, out: &mut impl Write) {
    for n in std::mem::take(&mut engine.notifications) {
        writeln!(
            out,
            "notify {} {} {} {} {:016x} {:016x} {:016x}",
            n.identifier,
            n.subtitle.as_deref().unwrap_or("-"),
            bits(n.range),
            n.looping,
            n.position[0].to_bits(),
            n.position[1].to_bits(),
            n.position[2].to_bits()
        )
        .expect("write");
    }
    let [used, streaming] = engine.used_channels();
    writeln!(
        out,
        "  Sounds: {used}/{} + {streaming}/{}",
        engine.limits[0], engine.limits[1]
    )
    .expect("write");
}

pub fn result_name(r: PlayResult) -> &'static str {
    match r {
        PlayResult::Started => "STARTED",
        PlayResult::StartedSilently => "STARTED_SILENTLY",
        PlayResult::NotStarted => "NOT_STARTED",
    }
}
