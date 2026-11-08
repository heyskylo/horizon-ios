"use client";

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import type { TickRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import { Spark } from "./spark";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function TicksPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<TickRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<TickRow>(capture.handle, {
      op: "ticks",
      file: capture.name,
      filters: { side: capture.side, limit: 1000 },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  const stat = stats(rows.map((row) => row.wall_ms));
  const worst = [...rows].sort((a, b) => b.wall_ms - a.wall_ms).slice(0, 8);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.summary}>
      <div className={styles.cards}>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>Min</h3>
          <p className={styles.statValue}>{num(stat?.min ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>Median</h3>
          <p className={styles.statValue}>{num(stat?.median ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>p95</h3>
          <p className={styles.statValue}>{num(stat?.p95 ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>Max</h3>
          <p className={styles.statValue}>{num(stat?.max ?? null, 2)} ms</p>
        </div>
      </div>
      <Spark values={rows.map((row) => row.wall_ms)} label="tick wall" width={600} height={80} />
      <div className={styles.ticks}>
        {worst.map((tick) => (
          <div key={tick.id} className={styles.tickHead}>
            <span className={styles.tickMs}>{num(tick.wall_ms, 2)} ms</span>
            <span className={styles.tickAt}>{num(tick.t, 1)} s</span>
          </div>
        ))}
      </div>
    </div>
  );
}
