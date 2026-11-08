"use client";

import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { SpanRow } from "@/lib/types";
import { useChanges } from "./comparison";
import { Delta, NewRows } from "./delta";
import { RowTools } from "./row-tools";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

const VISIBLE_MS = 0.005;

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function SpansPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<SpanRow>(reader, capture, "spans", { limit: 200 });
  const compare = useChanges();

  const max = rows.reduce((m, row) => Math.max(m, row.values[0] ?? 0), 0) || 1;

  const shown = rows.filter((row) => (row.values[0] ?? 0) >= VISIBLE_MS);
  const childrenOf = new Map<string, SpanRow[]>();
  for (const row of shown) {
    if (row.parent) childrenOf.set(row.parent, [...(childrenOf.get(row.parent) ?? []), row]);
  }

  function render(nodes: SpanRow[], depth = 0): ReactNode {
    return nodes.map((row) => {
      const children = childrenOf.get(row.id) ?? [];
      const value = row.values[0] ?? 0;
      return (
        <div key={row.id}>
          <div className={styles.meterRow}>
            <span className={styles.meterName} title={row.name} style={{ paddingLeft: depth * 14 }}>
              <span className={styles.nameText}>{row.name}</span>
              <RowTools id={row.id} name={row.name} />
            </span>
            <span className={styles.meterTrack} aria-hidden="true">
              <span className={styles.meterFill} style={{ width: `${(value / max) * 100}%` }} />
            </span>
            <span className={styles.meterValue}>{num(value, 2)} ms</span>
            {compare ? <Delta row={compare.changes.get(row.id)} /> : null}
            <span className={styles.meterExtra}>{grouped(row.seen[0] ?? 0)}</span>
          </div>
          {children.length ? render(children, depth + 1) : null}
        </div>
      );
    });
  }

  const roots = shown.filter((row) => !row.parent);
  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <>
      <div className={styles.meterList} data-delta={compare ? "value" : undefined}>
        <div className={styles.meterHead}>
          <span>{t("spans.name")}</span>
          <span />
          <span>{t("summary.msPerTick")}</span>
          {compare ? <span>{t("compare.columnChange")}</span> : null}
          <span>{t("spans.seen")}</span>
        </div>
        {render(roots.length ? roots : shown)}
        {rows.length > shown.length ? (
          <p className={styles.note}>{t("spans.hidden", { count: rows.length - shown.length })}</p>
        ) : null}
      </div>
      {compare ? <NewRows rows={compare.rows} prefix="s:" shown={new Set(rows.map((row) => row.id))} /> : null}
    </>
  );
}
