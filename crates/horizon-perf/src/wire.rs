//! What replies and the command line share: closed vocabularies as enums, the envelope, and rounding.

use horizon_perf_format::v1 as proto;
use serde::{Deserialize, Serialize};

/// A string that names none of an enum's variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownName;

/// An enum whose variants have a wire spelling (and optional aliases the parser also takes): serde, `Display`, `FromStr`.
macro_rules! vocab {
    ($(#[$meta:meta])* $name:ident { $( $(#[$vm:meta])* $variant:ident = $wire:literal $(| $alias:literal)* ),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize, ::serde::Deserialize,
        )]
        pub enum $name {
            $( $(#[$vm])* #[serde(rename = $wire $(, alias = $alias)*)] $variant ),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            /// Every spelling the command line takes, aliases included.
            #[allow(dead_code)]
            pub const SPELLINGS: &'static [&'static str] = &[$($wire $(, $alias)*),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $wire),+ }
            }

            #[allow(dead_code)]
            pub fn spellings() -> String {
                Self::SPELLINGS.join(", ")
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::wire::UnknownName;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $( $wire $(| $alias)* => Ok($name::$variant), )+
                    _ => Err($crate::wire::UnknownName),
                }
            }
        }
    };
}

pub(crate) use vocab;

/// `From` a prost enum with the same variant names, checked exhaustively.
macro_rules! mirror {
    ($name:ident <- $proto:ident { $($variant:ident),+ $(,)? }) => {
        impl From<proto::$proto> for $name {
            fn from(value: proto::$proto) -> Self {
                match value { $( proto::$proto::$variant => $name::$variant ),+ }
            }
        }
    };
}

vocab! {
    /// The `op` of a query: `samples` is listed but not answered yet.
    Op {
        Spans = "spans",
        Ticks = "ticks",
        Tick = "tick",
        Series = "series",
        World = "world",
        Net = "net",
        Meta = "meta",
        Memory = "memory",
        Marks = "marks",
        Warnings = "warnings",
        Hot = "hot",
        Tree = "tree",
        Callers = "callers",
        Callees = "callees",
        Split = "split",
        Group = "group",
        Frames = "frames",
        Gpu = "gpu",
        Bench = "bench",
        Samples = "samples",
    }
}

impl Op {
    /// Ops no piece answers yet: `samples` waits for its section.
    pub fn unanswered(self) -> bool {
        match self {
            Op::Samples => true,
            Op::Spans
            | Op::Ticks
            | Op::Tick
            | Op::Series
            | Op::World
            | Op::Net
            | Op::Gpu
            | Op::Meta
            | Op::Memory
            | Op::Marks
            | Op::Warnings
            | Op::Hot
            | Op::Tree
            | Op::Callers
            | Op::Callees
            | Op::Split
            | Op::Group
            | Op::Frames
            | Op::Bench => false,
        }
    }

    /// Ops that read the samples section.
    pub fn reads_samples(self) -> bool {
        matches!(
            self,
            Op::Hot | Op::Tree | Op::Callers | Op::Callees | Op::Split | Op::Group | Op::Frames
        )
    }

    pub fn answered() -> String {
        let names: Vec<&str> = Op::ALL
            .iter()
            .filter(|op| !op.unanswered())
            .map(|op| op.as_str())
            .collect();
        names.join(", ")
    }
}

vocab! {
    /// What `diff` echoes as its `op`.
    DiffOp { Diff = "diff" }
}

/// The `op` an envelope names: a query op, `diff`, or what the caller typed when it is neither.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpName {
    Query(Op),
    Diff(DiffOp),
    Other(String),
}

vocab! {
    /// `--select`: which time of a span to rank by.
    Select {
        Cpu = "cpu",
        Running = "running",
        Waiting = "waiting",
        Wall = "wall",
        Alloc = "alloc",
        Live = "live",
    }
}

vocab! {
    /// `--where` keys: what a filter narrows.
    ContextKey {
        Span = "span",
        Entity = "entity",
        Block = "block",
        Chunk = "chunk",
        Dim = "dim" | "dimension",
    }
}

vocab! {
    /// `--by`: how `world` groups its rows.
    GroupBy {
        Dimension = "dimension",
        Entity = "entity",
        BlockEntity = "block_entity",
        Cost = "cost",
    }
}

vocab! {
    /// What a reply's rows are measured in, or what they list.
    Unit {
        MsPerTick = "ms_per_tick",
        MsPerFrame = "ms_per_frame",
        MsPerSlowTick = "ms_per_slow_tick",
        MsPerSlowFrame = "ms_per_slow_frame",
        BytesPerTick = "bytes_per_tick",
        BytesPerFrame = "bytes_per_frame",
        BytesPerSlowTick = "bytes_per_slow_tick",
        BytesPerSlowFrame = "bytes_per_slow_frame",
        KibPerS = "kib_per_s",
        Count = "count",
        Series = "series",
        Meta = "meta",
        Marks = "marks",
        Warnings = "warnings",
        Memory = "memory",
        MemoryDiff = "memory_diff",
        Bench = "bench",
    }
}

vocab! {
    /// `hot --by`: which time ranks the functions.
    HotBy {
        SelfTime = "self",
        Total = "total",
    }
}

vocab! {
    /// `split --by` and `group --by`: the context a weight is divided by; `group` takes all but the last two.
    SampleBy {
        Span = "span",
        Entity = "entity",
        Block = "block",
        Chunk = "chunk",
        Dim = "dim" | "dimension",
        Thread = "thread",
        Mod = "mod",
        State = "state",
        Slow = "slow",
    }
}

impl SampleBy {
    /// Whether `group` takes it: `state` and `slow` only divide one node's weight.
    pub fn groups(self) -> bool {
        !matches!(self, SampleBy::State | SampleBy::Slow)
    }
}

vocab! {
    /// The state of a sample.
    StateName {
        Cpu = "cpu",
        Running = "running",
        Waiting = "waiting",
    }
}

impl StateName {
    /// The state a weights row's `state` number stands for; none for a number the format does not define.
    pub fn from_weight(state: u32) -> Option<Self> {
        match state {
            0 => Some(StateName::Cpu),
            1 => Some(StateName::Running),
            2 => Some(StateName::Waiting),
            _ => None,
        }
    }
}

vocab! {
    /// Whether a sample belongs to a slow tick or frame.
    PaceName {
        Slow = "slow",
        Normal = "normal",
    }
}

vocab! {
    MemoryUnit {
        Mb = "mb",
        Count = "count",
    }
}

vocab! {
    /// The unit of a health field in `series`.
    HealthUnit {
        TicksPerS = "ticks/s",
        Ms = "ms",
        FramesPerS = "frames/s",
        Cores = "cores",
        Mb = "MB",
        PerS = "per s",
        MibPerS = "MiB/s",
        Count = "count",
    }
}

/// The unit of a `series` field: a health field's, a span's, or a counter's own text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SeriesUnit {
    Health(HealthUnit),
    Span(Unit),
    Counter(String),
}

vocab! {
    /// How the second of two compared captures did against the first.
    Verdict {
        TooFewBlocks = "too_few_blocks",
        WithinNoise = "within_noise",
        Negligible = "negligible",
        Slower = "slower",
        Faster = "faster",
        New = "new",
        Gone = "gone",
    }
}

vocab! {
    /// Which of two compared captures or reports.
    Role {
        A = "a",
        B = "b",
    }
}

vocab! {
    BenchKind { Benchmark = "benchmark" }
}

vocab! {
    /// A tick on a server, a frame on a client.
    UnitKind {
        Tick = "tick",
        Frame = "frame",
    }
}

impl From<proto::Unit> for UnitKind {
    fn from(unit: proto::Unit) -> Self {
        match unit {
            proto::Unit::Frame => UnitKind::Frame,
            proto::Unit::Tick | proto::Unit::Unspecified => UnitKind::Tick,
        }
    }
}

vocab! {
    SideName {
        Server = "server",
        Client = "client",
        Unspecified = "unknown",
    }
}
mirror!(SideName <- Side { Server, Client, Unspecified });

vocab! {
    KindName {
        Dedicated = "dedicated",
        Integrated = "integrated",
        Client = "client",
        Unspecified = "unknown",
    }
}
mirror!(KindName <- Kind { Dedicated, Integrated, Client, Unspecified });

vocab! {
    TriggerName {
        Unspecified = "unspecified",
        Console = "console",
        Ctl = "ctl",
        Key = "key",
        Env = "env",
        Benchmark = "benchmark",
        Remote = "remote",
    }
}
mirror!(TriggerName <- Trigger { Unspecified, Console, Ctl, Key, Env, Benchmark, Remote });

vocab! {
    SymbolsName {
        Unspecified = "unspecified",
        Dwarf = "dwarf",
        Symtab = "symtab",
        None = "none",
    }
}
mirror!(SymbolsName <- Symbols { Unspecified, Dwarf, Symtab, None });

vocab! {
    ThreadSelectionName {
        Unspecified = "unspecified",
        Main = "main",
        All = "all",
    }
}
mirror!(ThreadSelectionName <- ThreadSelection { Unspecified, Main, All });

vocab! {
    GpuModeName {
        Unspecified = "unspecified",
        Off = "off",
        Passes = "passes",
        Detail = "detail",
    }
}
mirror!(GpuModeName <- GpuMode { Unspecified, Off, Passes, Detail });

vocab! {
    ModKindName {
        Unspecified = "unspecified",
        Content = "content",
        Native = "native",
    }
}
mirror!(ModKindName <- ModKind { Unspecified, Content, Native });

vocab! {
    StopReasonName {
        Unspecified = "unspecified",
        Normal = "normal",
        WindowCap = "window_cap",
        Disconnected = "disconnected",
        Shutdown = "shutdown",
    }
}
mirror!(StopReasonName <- StopReason { Unspecified, Normal, WindowCap, Disconnected, Shutdown });

vocab! {
    /// A warning's code, spelled as the proto names it.
    WarningCodeName {
        Unspecified = "WARNING_CODE_UNSPECIFIED",
        ServerNotHorizon = "WARNING_CODE_SERVER_NOT_HORIZON",
        ServerDenied = "WARNING_CODE_SERVER_DENIED",
        ServerAlreadyRunning = "WARNING_CODE_SERVER_ALREADY_RUNNING",
        ServerPartIncomplete = "WARNING_CODE_SERVER_PART_INCOMPLETE",
        TimelineCapped = "WARNING_CODE_TIMELINE_CAPPED",
        GpuUnavailable = "WARNING_CODE_GPU_UNAVAILABLE",
    }
}
mirror!(
    WarningCodeName <- WarningCode {
        Unspecified,
        ServerNotHorizon,
        ServerDenied,
        ServerAlreadyRunning,
        ServerPartIncomplete,
        TimelineCapped,
        GpuUnavailable
    }
);

vocab! {
    GpuBackendName {
        Unspecified = "unspecified",
        Vulkan = "vulkan",
        Metal = "metal",
        Dx12 = "dx12",
        Gl = "gl",
        Other = "other",
    }
}
mirror!(GpuBackendName <- GpuBackend { Unspecified, Vulkan, Metal, Dx12, Gl, Other });

vocab! {
    GpuPartsModeName {
        Unspecified = "unspecified",
        InsidePasses = "inside_passes",
        SeparatePasses = "separate_passes",
    }
}
mirror!(GpuPartsModeName <- GpuPartsMode { Unspecified, InsidePasses, SeparatePasses });

vocab! {
    GpuFragmentUnitName {
        Unspecified = "unspecified",
        Pixel = "pixel",
        Quad = "quad",
    }
}
mirror!(GpuFragmentUnitName <- GpuFragmentUnit { Unspecified, Pixel, Quad });

vocab! {
    GpuSourceName {
        Unspecified = "unspecified",
        Horizon = "horizon",
        ShaderPack = "shader_pack",
    }
}
mirror!(GpuSourceName <- GpuSource { Unspecified, Horizon, ShaderPack });

/// A number JSON writes as an integer when it is whole and small, else as a rounded decimal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    Int(i64),
    Float(f64),
}

/// Rounds to 3 decimals as spec A does; a number JSON cannot write serializes as null.
pub fn num(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Six decimals, for shares that are a fraction of a percent.
pub fn fine(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

pub fn opt_num(v: Option<f64>) -> Option<f64> {
    v.map(num)
}

/// A window end: rounded, or None when it is infinite.
pub fn bound(v: f64) -> Option<f64> {
    v.is_finite().then(|| num(v))
}

/// A count a sample holds as a float: whole numbers stay integers.
pub fn count(v: f64) -> Amount {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        Amount::Int(v as i64)
    } else {
        Amount::Float(num(v))
    }
}

/// An id and the name it stands for, offered when a name was unknown or ambiguous.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub name: String,
}

impl Candidate {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// One capture a query read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureInfo {
    pub file: String,
    pub sides: Vec<SideName>,
    pub kind: KindName,
    pub seconds: f64,
}

/// A `--where` value: one text, or several when the key was given more than once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl From<&[String]> for OneOrMany {
    fn from(values: &[String]) -> Self {
        match values {
            [one] => OneOrMany::One(one.clone()),
            many => OneOrMany::Many(many.to_vec()),
        }
    }
}

/// What a query looked at, so a reader can tell what its rows cover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub side: Option<SideName>,
    pub thread: Option<String>,
    pub select: Select,
    pub slow: bool,
    #[serde(rename = "where")]
    pub where_: std::collections::BTreeMap<ContextKey, OneOrMany>,
    /// Null when the window is unbounded on that end.
    pub from: Option<f64>,
    pub to: Option<f64>,
    pub ticks: Option<u64>,
    pub samples: Option<u64>,
}

/// The one shape of every reply (docs/perf-platform.md §6.3); a failure carries `error` and no rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope<R, C = CaptureInfo, S = Selection> {
    pub ok: bool,
    pub op: OpName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<C>,
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub selection: Option<S>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Unit>,
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<R>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
}

fn none<T>() -> Option<T> {
    None
}

impl<R, C, S> Envelope<R, C, S> {
    pub fn success(
        op: OpName,
        captures: Vec<C>,
        selection: Option<S>,
        unit: Unit,
        rows: Vec<R>,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            ok: true,
            op,
            error: None,
            candidates: Vec::new(),
            captures,
            selection,
            unit: Some(unit),
            rows: Some(rows),
            warnings: Some(warnings),
        }
    }

    pub fn failure(op: OpName, error: impl Into<String>, candidates: Vec<Candidate>) -> Self {
        Self {
            ok: false,
            op,
            error: Some(error.into()),
            candidates,
            captures: Vec::new(),
            selection: None,
            unit: None,
            rows: None,
            warnings: None,
        }
    }

    /// The rows of a reply that has them.
    pub fn rows(&self) -> &[R] {
        self.rows.as_deref().unwrap_or_default()
    }
}
