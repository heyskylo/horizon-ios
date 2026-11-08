"use client";

import { useEffect, useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import type { TickRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import { topSpans } from "@/lib/overview";
import { Spark } from "./spark";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function TicksPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<TickRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  // keep same query shape as before
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
  const selectedTick = rows.find((row) => row.id === selected) ?? null;

  const histogram = useMemo(() => {
    // 10 buckets over the observed range, wasm-less binning is fine for display.
    if (rows.length === 0) return [];
    const min = Math.min(...rows.map((r) => r.wall_ms));
    const max = Math.max(...rows.map((r) => r.wall_ms));
    const span = max - min || 1;
    const buckets = Array.from({ length: 10 }, () => 0);
    for (const row of rows) {
      const idx = Math.min(9, Math.floor(((row.wall_ms - min) / span) * 10));
      buckets[idx] += 1;
    }
    return buckets;
  }, [rows]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.summary}>
      <div className={styles.cards}>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>{t("ticks.min")}</h3>
          <p className={styles.statValue}>{num(stat?.min ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>{t("ticks.median")}</h3>
          <p className={styles.statValue}>{num(stat?.median ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>{t("ticks.p95")}</h3>
          <p className={styles.statValue}>{num(stat?.p95 ?? null, 2)} ms</p>
        </div>
        <div className={styles.stat}>
          <h3 className={styles.statTitle}>{t("ticks.max")}</h3>
          <p className={styles.statValue}>{num(stat?.max ?? null, 2)} ms</p>
        </div>
      </div>
      <Spark values={rows.map((row) => row.wall_ms)} label="tick wall" width={600} height={80} />
      <div style={{ display: "flex", gap: 2, alignItems: "end", height: 60, margin: "8px 0" }}>
        {histogram.map((count, idx) => (
          <div
            key={idx}
            title={String(count)}
            style={{
              flex: 1,
              background: "var(--accent, #888)",
              height: `${(count / Math.max(1, Math.max(...histogram))) * 100}%`,
              opacity: 0.8,
            }}
          />
        ))}
      </div>
      <div className={styles.ticks}>
        {worst.map((tick) => (
          <button
            key={tick.id}
            type="button"
            className={styles.tickHead}
            onClick={() => setSelected(tick.id)}
            style={{ border: 0, background: "transparent", cursor: "pointer", width: "100%", textAlign: "left" }}
          >
            <span className={styles.tickMs}>{num(tick.wall_ms, 2)} ms</span>
            <span className={styles.tickAt}>{num(tick.t, 1)} s</span>
          </button>
        ))}
      </div>
      {selectedTick ? (
        <div className={styles.meterList}>
          {topSpans(selectedTick).map((span, idx) => (
            <div key={idx} className={styles.meterRow}>
              <span className={styles.meterName}>{span[0]}</span>
              <span className={styles.meterValue}>{num(span[2], 2)} ms</span>
              <span className={styles.meterExtra}>{num(span[3], 2)} cpu</span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
