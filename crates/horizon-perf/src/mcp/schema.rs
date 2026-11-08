//! The JSON Schema of each tool's arguments, as the typed value `tools/list` serializes.

use std::collections::BTreeMap;

use serde::Serialize;

use super::tools::ToolName;
use crate::wire::{ContextKey, Op, Select, SideName, vocab};

vocab! {
    /// The JSON types the schemas use.
    JsonType {
        Object = "object",
        String = "string",
        Number = "number",
        Integer = "integer",
        Boolean = "boolean",
        Array = "array",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Schema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<JsonType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'static str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<&'static str, Schema>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,
    #[serde(rename = "enum", skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<&'static str>,
    #[serde(rename = "anyOf", skip_serializing_if = "Vec::is_empty")]
    pub any_of: Vec<Schema>,
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional: Option<bool>,
}

impl Schema {
    fn of(kind: JsonType, description: &'static str) -> Self {
        Self {
            kind: Some(kind),
            description: (!description.is_empty()).then_some(description),
            properties: BTreeMap::new(),
            required: Vec::new(),
            items: None,
            choices: Vec::new(),
            any_of: Vec::new(),
            additional: None,
        }
    }

    fn string(description: &'static str) -> Self {
        Self::of(JsonType::String, description)
    }

    fn number(description: &'static str) -> Self {
        Self::of(JsonType::Number, description)
    }

    fn integer(description: &'static str) -> Self {
        Self::of(JsonType::Integer, description)
    }

    fn boolean(description: &'static str) -> Self {
        Self::of(JsonType::Boolean, description)
    }

    fn strings(description: &'static str) -> Self {
        let mut array = Self::of(JsonType::Array, description);
        array.items = Some(Box::new(Self::of(JsonType::String, "")));
        array
    }

    fn one_of(mut self, choices: impl IntoIterator<Item = &'static str>) -> Self {
        self.choices = choices.into_iter().collect();
        self
    }

    fn object(
        description: &'static str,
        properties: impl IntoIterator<Item = (&'static str, Schema)>,
        required: &[&'static str],
    ) -> Self {
        let mut object = Self::of(JsonType::Object, description);
        object.properties = properties.into_iter().collect();
        object.required = required.to_vec();
        object.additional = Some(false);
        object
    }
}

fn side() -> Schema {
    Schema::string("which side of the capture")
        .one_of([SideName::Server, SideName::Client].map(SideName::as_str))
}

fn seconds(description: &'static str) -> Schema {
    let mut bound = Schema::of(JsonType::Number, description);
    bound.kind = None;
    bound.any_of = vec![
        Schema::of(JsonType::Number, "seconds; negative counts from the end"),
        Schema::of(JsonType::String, "seconds as text, or mark:NAME"),
    ];
    bound
}

/// The filters of `perf_query`, one property per `QueryRequest` field.
fn filters() -> Schema {
    let keys = ContextKey::ALL
        .iter()
        .map(|key| (key.as_str(), Schema::strings("values; several are OR")));
    Schema::object(
        "the query's filters; each is the --flag of `horizon-perf query` of the same name",
        [
            ("side", side()),
            (
                "thread",
                Schema::string("thread name; default the side's main thread"),
            ),
            (
                "select",
                Schema::string("sample kind and state; default cpu")
                    .one_of(Select::ALL.iter().map(|s| s.as_str())),
            ),
            (
                "slow",
                Schema::boolean("only samples and spans of slow ticks or frames"),
            ),
            (
                "where",
                Schema::object("AND of context keys narrowing the samples", keys, &[]),
            ),
            ("from", seconds("start of the window")),
            ("to", seconds("end of the window")),
            ("limit", Schema::integer("rows; default 25")),
            ("names", Schema::strings("spans: only these span names")),
            ("worst", Schema::integer("ticks: how many of the worst")),
            (
                "over",
                Schema::number("ticks: only those over this many ms"),
            ),
            ("every", Schema::number("series: resample step in seconds")),
            (
                "diff",
                Schema::boolean("memory: diff two snapshots (args: LABEL_A LABEL_B)"),
            ),
            (
                "min_mb",
                Schema::number("memory: hide changes under this many MB"),
            ),
            (
                "by",
                Schema::string("hot: self or total; world, split, group: the grouping"),
            ),
            (
                "match",
                Schema::string("hot: functions whose name contains this"),
            ),
            ("depth", Schema::integer("tree, callers, callees: how deep")),
            (
                "min_pct",
                Schema::number("tree: hide nodes under this percent"),
            ),
            (
                "self_share",
                Schema::boolean("tree: give each row its own time as a percent"),
            ),
            (
                "stats",
                Schema::boolean("series: add each series' min, mean, median, p95, max and last"),
            ),
        ],
        &[],
    )
}

pub fn input(tool: ToolName) -> Schema {
    match tool {
        ToolName::Summary => Schema::object(
            "",
            [
                (
                    "file",
                    Schema::string("a .hzperf capture; default the newest finished one"),
                ),
                (
                    "baseline",
                    Schema::string("an earlier .hzperf capture to compare against"),
                ),
                (
                    "budget",
                    Schema::integer("size of the summary in tokens; default 1500"),
                ),
                ("side", side()),
                (
                    "thread",
                    Schema::string("the span thread of the phase table"),
                ),
                (
                    "slow",
                    Schema::boolean("phases of the slow ticks or frames only"),
                ),
            ],
            &[],
        ),
        ToolName::Query => Schema::object(
            "",
            [
                (
                    "op",
                    Schema::string("the query").one_of(
                        Op::ALL
                            .iter()
                            .filter(|op| !op.unanswered())
                            .map(|op| op.as_str()),
                    ),
                ),
                (
                    "args",
                    Schema::strings(
                        "what follows the op that is not a file: ids, names, field names, labels",
                    ),
                ),
                (
                    "files",
                    Schema::strings(
                        ".hzperf captures; default the newest finished one; spans and frames take several",
                    ),
                ),
                ("filters", filters()),
            ],
            &["op"],
        ),
        ToolName::Diff => Schema::object(
            "",
            [
                (
                    "a",
                    Schema::string("the earlier .hzperf capture, or benchmark report .json"),
                ),
                ("b", Schema::string("the later one, of the same kind")),
                (
                    "min_ms",
                    Schema::number("leave out rows under this in both"),
                ),
                ("side", side()),
                ("thread", Schema::string("the span thread compared")),
                ("limit", Schema::integer("rows; default 25")),
            ],
            &["a", "b"],
        ),
        ToolName::List => Schema::object(
            "",
            [(
                "dir",
                Schema::string("a folder of captures; default the game's reports folder"),
            )],
            &[],
        ),
        ToolName::Follow => Schema::object(
            "",
            [
                (
                    "file",
                    Schema::string(
                        "a .hzperf capture, finished or still being written; default the newest",
                    ),
                ),
                (
                    "cursor",
                    Schema::integer("the cursor of the last reply; omit to read from the start"),
                ),
            ],
            &[],
        ),
    }
}
