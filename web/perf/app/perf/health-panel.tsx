"use client";

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { byField, fieldsFor, stats, stepFor } from "@/lib/health";
import type { Stats } from "@/lib/health";
import { withUnit, num, grouped, mib } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { SeriesRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import { Spark } from "./spark";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

const WINDOWS: [string, number][] = [
  ["health.window1m", 60],
  ["health.window5m", 300],
  ["health.window15m", 900],
];

function StatCard({
  title,
  value,
  unit,
  rows,
  children,
}: {
  title: string;
  value: string;
  unit?: string;
  rows: [string, string][];
  children?: ReactNode;
}) {
  return (
    <section className={styles.stat}>
      <h3 className={styles.statTitle}>{title}</h3>
      <p className={styles.statValue}>
        {value}
        {unit ? <span className={styles.statUnit}>{unit}</span> : null}
      </p>
      {children}
      <dl className={styles.statRows}>
        {rows.map(([label, text]) => (
          <div key={label} className={styles.statRow}>
            <dt>{label}</dt>
            <dd>{text}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/** Health bar and timelines (docs/perf-platform.md §7.2, UI1). Every value is a wasm `series` value. */
export function HealthPanel({ reader, capture }: Props) {
  const [series, setSeries] = useState<SeriesRow[]>([]);
  const [trailing, setTrailing] = useState<[string, number | null][]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    setSeries([]);
    setTrailing([]);
    const meta = capture.meta.rows.find((row) => row.side === capture.side);
    const seconds = meta?.seconds ?? 0;
    const reply = reader.query<SeriesRow>(capture.handle, {
      op: "series",
      args: fieldsFor(capture.side),
      file: capture.name,
      filters: { side: capture.side, every: stepFor(seconds) },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      return;
    }
    setSeries(reply.rows);
    if (capture.side !== "server" || seconds <= 0) return;
    setTrailing(
      WINDOWS.map(([label, window]) => {
        const from = Math.max(0, seconds - window);
        const reply = reader.query<SeriesRow>(capture.handle, {
          op: "series",
          args: ["h:tps"],
          file: capture.name,
          filters: { side: capture.side, from, every: Math.min(window, seconds) },
        });
        const value = reply.ok ? (reply.rows[0]?.values[0] ?? null) : null;
        return [t(label), value] as [string, number | null];
      }),
    );
  }, [reader, capture]);

  const fields = byField(series);
  const get = (name: string): Stats | null => stats(fields.get(name)?.values ?? []);

  const sections = capture.meta.rows.find((row) => row.side === capture.side)?.sections ?? {};
  const has = (name: string) => (sections[name] ?? 0) > 0;

  if (error) return <p className={styles.error}>{error}</p>;
  if (!has("health") && !has("history")) {
    return <p className={styles.empty}>{t("health.empty")}</p>;
  }

  const cards: ReactNode[] = [];
  if (capture.side === "server") {
    const tps = get("h:tps");
    const avg = get("h:mspt_avg");
    const worst = get("h:mspt_p95");
    const peak = get("h:mspt_max");
    cards.push(
      <StatCard
        key="tps"
        title={t("health.tps")}
        value={num(tps?.last ?? null, 1)}
        unit={t("health.unitTps")}
        rows={trailing.map(([label, value]) => [label, num(value, 1)])}
      >
        <Spark values={fields.get("h:tps")?.values ?? []} label={t("health.tps")} />
      </StatCard>,
      <StatCard
        key="mspt"
        title={t("health.mspt")}
        value={num(avg?.mean ?? null, 1)}
        unit={t("health.unitMs")}
        rows={[
          [t("health.min"), num(avg?.min ?? null, 1)],
          [t("health.median"), num(avg?.median ?? null, 1)],
          [t("health.worstP95"), num(worst?.max ?? null, 1)],
          [t("health.max"), num(peak?.max ?? null, 1)],
        ]}
      >
        <Spark values={fields.get("h:mspt_p95")?.values ?? []} label={t("health.mspt")} />
      </StatCard>,
    );
  } else {
    const fps = get("h:fps");
    const p50 = get("h:frame_ms_p50");
    const p95 = get("h:frame_ms_p95");
    const p99 = get("h:frame_ms_p99");
    const peak = get("h:frame_ms_max");
    const cpu = get("h:frame_cpu_ms_p50");
    const gpu = get("h:frame_gpu_ms_p50");
    cards.push(
      <StatCard
        key="fps"
        title={t("health.fps")}
        value={num(fps?.last ?? null, 1)}
        unit={t("health.unitFps")}
        rows={[
          [t("health.min"), num(fps?.min ?? null, 1)],
          [t("health.median"), num(fps?.median ?? null, 1)],
          [t("health.max"), num(fps?.max ?? null, 1)],
        ]}
      >
        <Spark values={fields.get("h:fps")?.values ?? []} label={t("health.fps")} />
      </StatCard>,
      <StatCard
        key="frame"
        title={t("health.frame")}
        value={num(p95?.median ?? null, 1)}
        unit={t("health.unitMs")}
        rows={[
          [t("health.p50"), num(p50?.median ?? null, 1)],
          [t("health.p90"), num(p99?.median ?? null, 1)],
          [t("health.max"), num(peak?.max ?? null, 1)],
          [t("health.cpu"), num(cpu?.median ?? null, 1)],
          [t("health.gpu"), num(gpu?.median ?? null, 1)],
        ]}
      >
        <Spark values={fields.get("h:frame_ms_p95")?.values ?? []} label={t("health.frame")} />
      </StatCard>,
    );
  }

  const cpu = get("h:cpu_process_cores");
  const system = get("h:cpu_system_cores");
  const rss = get("h:rss_mb");
  const physical = get("h:physical_used_mb");
  const swap = get("h:swap_used_mb");
  const allocs = get("h:allocs_per_s");
  const allocRate = get("h:alloc_mib_per_s");
  const commit = get("h:allocator_commit_mb");

  cards.push(
    <StatCard
      key="cpu"
      title={t("health.cpuTitle")}
      value={num(cpu?.mean ?? null, 2)}
      unit={t("health.unitCores")}
      rows={[
        [t("health.process"), num(cpu?.mean ?? null, 2)],
        [t("health.system"), num(system?.mean ?? null, 2)],
      ]}
    >
      <Spark values={fields.get("h:cpu_process_cores")?.values ?? []} label={t("health.cpuTitle")} />
    </StatCard>,
    <StatCard
      key="memory"
      title={t("health.memory")}
      value={mib(rss?.last)}
      rows={[
        [t("health.resident"), mib(rss?.last)],
        [t("health.physical"), mib(physical?.last)],
        [t("health.swap"), mib(swap?.last)],
      ]}
    >
      <Spark values={fields.get("h:rss_mb")?.values ?? []} label={t("health.memory")} />
    </StatCard>,
    ...(allocs || allocRate || commit
      ? [
          <StatCard
            key="alloc"
            title={t("health.allocations")}
            value={withUnit(allocs?.mean, t("health.unitPerS"), 0)}
            rows={[
              [t("health.perSecond"), allocs?.mean === undefined ? t("meta.empty") : grouped(allocs.mean)],
              [t("health.allocated"), withUnit(allocRate?.mean, t("health.unitMibPerS"), 2)],
              [t("health.commit"), mib(commit?.last)],
            ]}
          />,
        ]
      : []),
  );

  if (capture.side === "server") {
    const players = get("h:players");
    const chunks = get("h:chunks");
    const entities = get("h:entities");
    cards.push(
      <StatCard
        key="world"
        title={t("health.world")}
        value={num(players?.last ?? null, 0)}
        unit={t("health.players")}
        rows={[
          [t("health.chunks"), num(chunks?.last ?? null, 0)],
          [t("health.entities"), num(entities?.last ?? null, 0)],
        ]}
      />,
    );
  }

  return (
    <div className={styles.health}>
      <div className={styles.cards}>{cards}</div>
      {series.length === 0 ? <p className={styles.empty}>{t("health.empty")}</p> : null}
      <p className={styles.note}>
        {t("health.note", { points: grouped(series[0]?.values.length ?? 0) })}
      </p>
    </div>
  );
}
