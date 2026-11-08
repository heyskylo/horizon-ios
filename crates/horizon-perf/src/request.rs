//! A query's filters as the command line and the MCP tool both state them, and the `Query` they become.

use std::collections::BTreeMap;
use std::str::FromStr;

use horizon_perf_format::v1::Side;
use serde::{Deserialize, Serialize};

use crate::query::{Bound, DEFAULT_LIMIT, Filters, Query};
use crate::wire::{ContextKey, GroupBy, HotBy, Op, SampleBy, Select, SideName};

/// A `from` or `to`: seconds, or text that is seconds or `mark:NAME`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoundArg {
    Seconds(f64),
    Text(String),
}

/// What is wrong with a request; the message names the filter.
#[derive(Clone, Debug, PartialEq)]
pub struct RequestError(pub String);

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RequestError {}

/// Every filter and option of `query` other than the op, the ids and the files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<SideName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<Select>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slow: bool,
    #[serde(default, rename = "where", skip_serializing_if = "BTreeMap::is_empty")]
    pub where_: BTreeMap<ContextKey, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<BoundArg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<BoundArg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worst: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub over: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diff: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_mb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub matching: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub self_share: bool,
}

fn bound(name: &str, arg: Option<BoundArg>) -> Result<Option<Bound>, RequestError> {
    match arg {
        None => Ok(None),
        Some(BoundArg::Seconds(s)) => Ok(Some(Bound::Seconds(s))),
        Some(BoundArg::Text(text)) => match text.strip_prefix("mark:") {
            Some(mark) => Ok(Some(Bound::Mark(mark.to_string()))),
            None => text.parse().map(|s| Some(Bound::Seconds(s))).map_err(|_| {
                RequestError(format!("{name} needs seconds or mark:NAME, got {text}"))
            }),
        },
    }
}

/// The side asked for; `unknown` is not one.
pub fn side(name: Option<SideName>) -> Result<Option<Side>, RequestError> {
    match name {
        None => Ok(None),
        Some(SideName::Server) => Ok(Some(Side::Server)),
        Some(SideName::Client) => Ok(Some(Side::Client)),
        Some(SideName::Unspecified) => Err(RequestError("side is client or server".into())),
    }
}

impl QueryRequest {
    /// The query for `op` with the ids `args`, or what is wrong with the filters.
    pub fn into_query(self, op: Op, args: Vec<String>) -> Result<Query, RequestError> {
        let usage =
            |what: &str, spellings: String| RequestError(format!("by is {spellings}, got {what}"));
        let (mut by, mut hot_by, mut by_sample) = (None, None, None);
        if let Some(text) = &self.by {
            match op {
                Op::Hot => {
                    hot_by =
                        Some(HotBy::from_str(text).map_err(|_| usage(text, HotBy::spellings()))?);
                }
                Op::Split | Op::Group => {
                    let picked = SampleBy::from_str(text)
                        .ok()
                        .filter(|b| op == Op::Split || b.groups());
                    by_sample = Some(picked.ok_or_else(|| {
                        let ok: Vec<&str> = SampleBy::ALL
                            .iter()
                            .filter(|b| op == Op::Split || b.groups())
                            .map(|b| b.as_str())
                            .collect();
                        usage(text, ok.join(", "))
                    })?);
                }
                _ => {
                    by = Some(
                        GroupBy::from_str(text).map_err(|_| usage(text, GroupBy::spellings()))?,
                    );
                }
            }
        }
        let filters = Filters {
            side: side(self.side)?,
            thread: self.thread,
            select: self.select.unwrap_or(Select::Cpu),
            slow: self.slow,
            where_: self.where_,
            from: bound("from", self.from)?,
            to: bound("to", self.to)?,
            limit: self.limit.unwrap_or(DEFAULT_LIMIT),
        };
        Ok(Query {
            op,
            args,
            filters,
            names: self.names,
            worst: self.worst,
            over: self.over,
            every: self.every,
            diff: self.diff,
            min_mb: self.min_mb,
            by,
            hot_by,
            by_sample,
            matching: self.matching,
            depth: self.depth,
            min_pct: self.min_pct,
            self_share: self.self_share,
        })
    }
}
