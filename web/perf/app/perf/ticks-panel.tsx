"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { at, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { spanId } from "@/lib/info";
import type { TickRow } from "@/lib/types";
import { Icons } from "./icons";
import { RowTools } from "./row-tools";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

/** Spans shorter than this are noise in a tick's tree. */
const VISIBLE_MS = 0.005;

/** A server tick over 50 ms misses the 20 ticks per second budget; the same line marks a frame under 20 FPS. */
const OVER_MS = 50;

function SlowTick({ tick, worst }: { tick: TickRow; worst: number }) {
  const spans = tick.spans.filter(([, , wall]) => wall >= VISIBLE_MS);
  return (
    <div className={styles.tickRow}>
      <details className={styles.tick}>
        <summary className={styles.tickHead}>
          <span className={styles.tickMs} data-over={tick.wall_ms > OVER_MS}>
            {num(tick.wall_ms, 1)} ms
          </span>
          <span className={styles.tickAt}>
            {tick.before_capture ? t("ticks.before") : t("summary.at", { time: at(tick.t) })}
          </span>
          <span className={styles.meterTrack} aria-hidden="true">
            <span className={styles.meterFill} style={{ width: `${(tick.wall_ms / worst) * 100}%` }} />
          </span>
          <span className={styles.tickChevron} aria-hidden="true">
            <Icons.chevronDown />
          </span>
        </summary>
        <div className={styles.tickBody}>
          <div className={styles.tickSpan} aria-hidden="true">
            <span className={styles.tickSpanName}>{t("spans.name")}</span>
            <span className={styles.tickSpanMs}>{t("ticks.wall")}</span>
            <span className={styles.tickSpanMs}>{t("ticks.cpu")}</span>
          </div>
          {spans.map(([name, depth, wall, cpu], index) => (
            <div key={`${name}-${index}`} className={styles.tickSpan}>
              <span className={styles.tickSpanName} style={{ paddingLeft: depth * 12 }}>
                <span className={styles.nameText}>{name}</span>
                <RowTools id={spanId(name)} name={name} />
              </span>
              <span className={styles.tickSpanMs}>{num(wall, 2)} ms</span>
              <span className={styles.tickSpanMs}>{num(cpu, 2)} ms</span>
            </div>
          ))}
        </div>
      </details>
      <span className={styles.tickTools}>
        <RowTools id={tick.id} name={at(tick.t)} />
      </span>
    </div>
  );
}

/** The slowest ticks (or frames) the capture kept, each opening into the spans that made it slow. */
export function TicksPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<TickRow>(reader, capture, "ticks", { worst: 30 });

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  const worst = rows[0].wall_ms || 1;
  return (
    <div className={styles.summary}>
      <header>
        <h3 className={styles.cardTitle}>
          {t(capture.side === "client" ? "ticks.headingFrames" : "ticks.headingTicks", { count: rows.length })}
        </h3>
        <p className={styles.cardNote}>{t(capture.side === "client" ? "ticks.noteFrames" : "ticks.note")}</p>
      </header>
      <div className={styles.ticks}>
        {rows.map((tick) => (
          <SlowTick key={tick.id} tick={tick} worst={worst} />
        ))}
      </div>
    </div>
  );
}
