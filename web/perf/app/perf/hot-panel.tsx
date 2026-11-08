"use client";

import { useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { GroupRow, HotRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import { FlameGraph } from "./flame-graph";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

type SortKey = "self" | "total";
type Mode = "flat" | "mods";

export function HotPanel({ reader, capture }: Props) {
  const [sort, setSort] = useState<SortKey>("total");
  const [topDown, setTopDown] = useState(true);
  const [limit, setLimit] = useState(250);
  const [mode, setMode] = useState<Mode>("flat");
  const [selected, setSelected] = useState<HotRow | null>(null);
  const { rows, error } = useRows<HotRow>(reader, capture, "hot", { limit, by: sort === "total" ? "total" : "self" });


  const ordered = useMemo(() => {
    const list = [...rows].sort((a, b) =>
      sort === "total" ? b.total - a.total : b.self - a.self,
    );
    return topDown ? list : [...list].reverse();
  }, [rows, sort, topDown]);

  const mods = useRows<GroupRow>(reader, capture, "group", { by: "mod", limit: 100 }, mode === "mods");
  const modRows = mods.rows;

  const modMax = useMemo(() => modRows.reduce((m, row) => Math.max(m, row.value), 0) || 1, [modRows]);

  const max = useMemo(() => (rows.reduce((m, row) => Math.max(m, sort === "total" ? row.total : row.self), 0) || 1), [rows, sort]);

  if (error ?? mods.error) return <p className={styles.error}>{error ?? mods.error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div>
      <div className={styles.toolbar}>
        <div className={styles.segmented} role="group" aria-label={t("hot.view")}>
          {(["flat", "mods"] as const).map((id) => (
            <button
              key={id}
              type="button"
              className={mode === id ? styles.segmentActive : styles.segment}
              aria-pressed={mode === id}
              onClick={() => setMode(id)}
            >
              {t(`hot.${id}`)}
            </button>
          ))}
        </div>
        {mode === "flat" ? (
          <>
            <div className={styles.segmented} role="group" aria-label={t("hot.sort")}>
              {(["total", "self"] as const).map((id) => (
                <button
                  key={id}
                  type="button"
                  className={sort === id ? styles.segmentActive : styles.segment}
                  aria-pressed={sort === id}
                  onClick={() => setSort(id)}
                >
                  {t(`hot.${id}`)}
                </button>
              ))}
            </div>
            <button type="button" className={styles.buttonSmall} onClick={() => setTopDown((v) => !v)}>
              {topDown ? t("hot.topDown") : t("hot.bottomUp")}
            </button>
            <select className={styles.select} aria-label={t("hot.limit")} value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
              <option value={250}>250</option>
              <option value={100}>100</option>
            </select>
          </>
        ) : null}
      </div>
      {mode === "flat" ? (
        <div className={styles.meterList}>
          <div className={styles.meterHead}>
            <span>{t("hot.function")}</span>
            <span />
            <span>{t("hot.self")}</span>
            <span>{t("hot.total")}</span>
          </div>
          {ordered.slice(0, 250).map((row) => (
            <div
              key={row.id}
              className={styles.meterRow}
              title={`${row.full} (${row.id})`}
              onClick={() => setSelected(row)}
            >
              <span className={styles.meterName}>{row.name}</span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span
                  className={styles.meterFill}
                  style={{ width: `${(((sort === "total" ? row.total : row.self) / max) * 100)}%` }}
                />
              </span>
              <span className={styles.meterValue}>{num(row.self, 2)} ms</span>
              <span className={styles.meterExtra}>{num(row.total, 2)} ms</span>
            </div>
          ))}
        </div>
      ) : (
        <div className={styles.meterList}>
          {modRows.map((row) => {
            const label = row.key === null ? t("hot.game") : row.label;
            return (
              <div key={row.key ?? ""} className={styles.meterRow} title={label}>
                <span className={styles.meterName}>{label}</span>
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
      {selected ? <FlameGraph rows={[selected]} /> : null}
    </div>
  );
}
