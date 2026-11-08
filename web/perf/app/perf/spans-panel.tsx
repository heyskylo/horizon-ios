"use client";

import { useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped, num } from "@/lib/format";
import { stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import type { SpanRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function SpansPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<SpanRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<SpanRow>(capture.handle, {
      op: "spans",
      file: capture.name,
      filters: { side: capture.side, limit: 200 },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  const max = rows.reduce((m, row) => Math.max(m, row.values[0] ?? 0), 0) || 1;

  function render(nodes: SpanRow[], depth = 0): ReactNode {
    return nodes.map((row) => {
      const children = rows.filter((candidate) => candidate.parent === row.id);
      const value = row.values[0] ?? 0;
      return (
        <div
          key={row.id}
          style={{ "--depth": depth, paddingLeft: "calc(var(--depth, 0) * 0.75rem)" } as CSSProperties}
        >
          <div className={styles.meterRow}>
            <span className={styles.meterName} title={row.name}>
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
