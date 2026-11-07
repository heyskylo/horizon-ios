use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{Ctx, QueryError, Reply, num};
use crate::ids::{Id, Kind};

/// Chunks, entities and block entities per dimension, or the sampled entity costs, from one snapshot (default the last).
pub(super) fn world(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    let view = &ctx.views[0];
    let snaps: Vec<_> = view.capture.world.iter().filter(|w| w.side() == side).collect();
    let label = ctx.q.args.first();
    let snap = match label {
        Some(label) => snaps.iter().find(|w| view.text(side, w.label) == label.as_str()),
        None => snaps.iter().max_by(|a, b| a.t.total_cmp(&b.t)),
    };
    let Some(snap) = snap else {
        let labels: Vec<String> = snaps.iter().map(|w| view.text(side, w.label).into_owned()).collect();
        return Err(QueryError::new(match label {
            Some(label) => format!("no world snapshot labelled {label}; labels: {}", labels.join(", ")),
            None => "this capture has no world section for the selected side".to_string(),
        }));
    };
    let wanted_dims: Vec<&str> = ctx
        .q
        .filters
        .where_
        .iter()
        .filter(|(k, _)| k == "dim" || k == "dimension")
        .map(|(_, v)| v.as_str())
        .collect();
    let dim_ok = |name: &str| wanted_dims.is_empty() || wanted_dims.contains(&name);
    let id = |kind: Kind, key: &str| Id::new(kind, key).to_string();
    let by = ctx.q.by.as_deref().unwrap_or("dimension");
    let mut rows: Vec<Value> = match by {
        "dimension" => {
            let mut dims: Vec<_> = snap.dims.iter().collect();
            dims.sort_by_key(|d| std::cmp::Reverse(d.chunks));
            dims
        }
        .into_iter()
            .map(|d| (view.text(side, d.dimension).into_owned(), d))
            .filter(|(name, _)| dim_ok(name))
            .map(|(name, d)| {
                json!({
                    "id": id(Kind::Dimension, &name),
                    "name": name,
                    "chunks": d.chunks,
                    "entities": d.entities.iter().map(|e| u64::from(e.count)).sum::<u64>(),
                    "block_entities": d.block_entities.iter().map(|e| u64::from(e.count)).sum::<u64>(),
                })
            })
            .collect(),
        "entity" | "block_entity" => {
            let mut counts: BTreeMap<String, u64> = BTreeMap::new();
            for d in snap.dims.iter().filter(|d| dim_ok(&view.text(side, d.dimension))) {
                let list = if by == "entity" { &d.entities } else { &d.block_entities };
                for e in list {
                    *counts.entry(view.text(side, e.r#type).into_owned()).or_default() += u64::from(e.count);
                }
            }
            let kind = if by == "entity" { Kind::Entity } else { Kind::Block };
            let mut ranked: Vec<_> = counts.into_iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            ranked
                .into_iter()
                .map(|(name, count)| json!({"id": id(kind, &name), "name": name, "count": count}))
                .collect()
        }
        "cost" => {
            let mut costs: Vec<_> = snap
                .entity_costs
                .iter()
                .filter(|c| dim_ok(&view.text(side, c.dimension)))
                .collect();
            costs.sort_by(|a, b| b.ms_per_tick.total_cmp(&a.ms_per_tick));
            costs
                .into_iter()
                .map(|c| {
                    let name = view.text(side, c.entity);
                    json!({
                        "id": id(Kind::Entity, &name),
                        "name": name,
                        "dimension": id(Kind::Dimension, &view.text(side, c.dimension)),
                        "ms_per_tick": num(f64::from(c.ms_per_tick)),
                        "mobs_ticked": num(f64::from(c.mobs_ticked_per_tick)),
                        "us_per_mob": num(f64::from(c.us_per_mob)),
                    })
                })
                .collect()
        }
        other => {
            return Err(QueryError::new(format!(
                "world --by is dimension, entity, block_entity or cost, got {other}"
            )));
        }
    };
    rows.truncate(ctx.q.filters.limit);
    let unit = if by == "cost" { "ms_per_tick" } else { "count" };
    let mut reply = Reply::new(unit, rows);
    if snaps.len() > 1 && label.is_none() {
        reply.warnings.push(format!(
            "this capture has {} world snapshots; showing {}",
            snaps.len(),
            view.text(side, snap.label)
        ));
    }
    Ok(reply)
}
