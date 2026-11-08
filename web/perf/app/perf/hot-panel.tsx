"use client";

import { useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { GroupRow, HotRow } from "@/lib/types";
import { RowTools } from "./row-tools";
import { Segmented } from "./segmented";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

type SortKey = "self" | "total";
type Mode = "flat" | "mods";

function largest(values: number[]): number {
  return values.reduce((a, b) => Math.max(a, b), 0) || 1;
}

export function HotPanel({ reader, capture }: Props) {
  const [sort, setSort] = useState<SortKey>("total");
  const [mode, setMode] = useState<Mode>("flat");
  const meta = capture.meta.rows.find((row) => row.side === capture.side);
  const hasMods = meta?.mods.some((item) => item.kind === "native") ?? false;
  const view: Mode = hasMods ? mode : "flat";

  const { rows, error } = useRows<HotRow>(reader, capture, "hot", { limit: 250, by: sort });
  const mods = useRows<GroupRow>(reader, capture, "group", { by: "mod", limit: 100 }, view === "mods");
  const max = largest(rows.map((row) => row[sort]));
  const modMax = largest(mods.rows.map((row) => row.value));

  const problem = error ?? mods.error;
  if (problem) return <p className={styles.error}>{problem}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div>
      <div className={styles.toolbar}>
        {hasMods ? (
          <Segmented
            label={t("hot.view")}
            options={[
              { id: "flat", text: t("hot.flat") },
              { id: "mods", text: t("hot.mods") },
            ]}
            value={view}
            onChange={setMode}
          />
        ) : null}
        {view === "flat" ? (
          <Segmented
            label={t("hot.sort")}
            options={[
              { id: "total", text: t("hot.total") },
              { id: "self", text: t("hot.self") },
            ]}
            value={sort}
            onChange={setSort}
          />
        ) : null}
      </div>
      {view === "flat" ? (
        <div className={styles.meterList}>
          <div className={styles.meterHead}>
            <span>{t("hot.function")}</span>
            <span />
            <span>{t("hot.self")}</span>
            <span>{t("hot.total")}</span>
          </div>
          {rows.map((row) => (
            <div key={row.id} className={styles.meterRow} title={row.full}>
              <span className={styles.meterName}>
                <span className={styles.nameText}>{row.name}</span>
                <RowTools id={row.id} name={row.name} />
              </span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span className={styles.meterFill} style={{ width: `${(row[sort] / max) * 100}%` }} />
              </span>
              <span className={styles.meterValue}>{num(row.self, 2)} ms</span>
              <span className={styles.meterExtra}>{num(row.total, 2)} ms</span>
            </div>
          ))}
        </div>
      ) : (
        <div className={styles.meterList}>
          {mods.rows.map((row) => {
            const label = row.key === null ? t("hot.game") : row.label;
            return (
              <div key={row.key ?? ""} className={styles.meterRow} title={label}>
                <span className={styles.meterName}>
                  <span className={styles.nameText}>{label}</span>
                  <RowTools id={row.key} name={label} />
                </span>
                <span className={styles.meterTrack} aria-hidden="true">
                  <span className={styles.meterFill} style={{ width: `${(row.value / modMax) * 100}%` }} />
                </span>
                <span className={styles.meterValue}>{num(row.value, 2)} ms</span>
                <span className={styles.meterExtra}>{num(row.pct, 1)}%</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
