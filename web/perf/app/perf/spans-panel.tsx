"use client";

import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped, num } from "@/lib/format";
import { stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import type { SpanRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function SpansPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<SpanRow>(reader, capture, "spans", { limit: 200 });


  const max = rows.reduce((m, row) => Math.max(m, row.values[0] ?? 0), 0) || 1;

  const childrenOf = new Map<string, SpanRow[]>();
  for (const row of rows) {
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
              {row.name}
            </span>
            <span className={styles.meterTrack} aria-hidden="true">
              <span className={styles.meterFill} style={{ width: `${(value / max) * 100}%` }} />
            </span>
            <span className={styles.meterValue}>{num(value, 2)} ms</span>
            <span className={styles.meterExtra}>{grouped(row.seen[0] ?? 0)}</span>
          </div>
          {children.length ? render(children, depth + 1) : null}
        </div>
      );
    });
  }

  const roots = rows.filter((row) => !row.parent);
  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.meterList}>
      <div className={styles.meterRow}>
        <span className={styles.meterName}>Span</span>
        <span />
        <span className={styles.meterValue}>ms/tick</span>
        <span className={styles.meterExtra}>seen</span>
      </div>
      {render(roots.length ? roots : rows)}
    </div>
  );
}
