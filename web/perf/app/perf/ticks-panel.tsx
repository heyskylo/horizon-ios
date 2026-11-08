"use client";

import { useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import type { TickRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import { topSpans } from "@/lib/overview";
import { Spark } from "./spark";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function TicksPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<TickRow>(reader, capture, "ticks", { limit: 1000 });
  const [selected, setSelected] = useState<string | null>(null);

  // keep same query shape as before

  const stat = stats(rows.map((row) => row.wall_ms));
  const worst = [...rows].sort((a, b) => b.wall_ms - a.wall_ms).slice(0, 8);
  const worstMs = worst[0]?.wall_ms || 1;
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
      <div className={styles.histogram}>
        {histogram.map((count, idx) => (
          <div
            key={idx}
            className={styles.histogramBar}
            title={String(count)}
            style={{ height: `${(count / Math.max(1, ...histogram)) * 100}%` }}
          />
        ))}
      </div>
      <div className={styles.ticks}>
        {worst.map((tick) => (
          <button
            key={tick.id}
            type="button"
            className={styles.tickHead}
            aria-pressed={tick.id === selected}
            onClick={() => setSelected(tick.id)}
          >
            <span className={styles.tickMs} data-over={tick.wall_ms > 50}>{num(tick.wall_ms, 2)} ms</span>
            <span className={styles.tickAt}>{num(tick.t, 1)} s</span>
            <span className={styles.meterTrack} aria-hidden="true">
              <span className={styles.meterFill} style={{ width: `${(tick.wall_ms / worstMs) * 100}%` }} />
            </span>
          </button>
        ))}
      </div>
      {selectedTick ? (
        <div className={styles.meterList}>
          {topSpans(selectedTick).map((span, idx) => (
            <div key={idx} className={styles.meterRow}>
              <span className={styles.meterName}>{span[0]}</span>
              <span />
              <span className={styles.meterValue}>{num(span[2], 2)} ms</span>
              <span className={styles.meterExtra}>{num(span[3], 2)} cpu</span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
