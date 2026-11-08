"use client";

import { useEffect, useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { HotRow } from "@/lib/types";
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
  const [rows, setRows] = useState<HotRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [sort, setSort] = useState<SortKey>("total");
  const [topDown, setTopDown] = useState(true);
  const [limit, setLimit] = useState(250);
  const [mode, setMode] = useState<Mode>("flat");
  const [selected, setSelected] = useState<HotRow | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<HotRow>(capture.handle, {
      op: "hot",
      file: capture.name,
      filters: { side: capture.side, limit, by: sort === "total" ? "total" : "self" },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture, sort, limit]);

  const ordered = useMemo(() => {
    const list = [...rows].sort((a, b) =>
      sort === "total" ? b.total - a.total : b.self - a.self,
    );
    return topDown ? list : [...list].reverse();
  }, [rows, sort, topDown]);

  const modRows = useMemo(() => {
    if (mode !== "mods") return [];
    const grouped = new Map<string, { total: number; self: number; samples: number }>();
    for (const row of rows) {
      // crate from the first path segment of the full name (Start.mods maps these).
      const crate = row.full.split("::")[0] || "unknown";
      const cur = grouped.get(crate) ?? { total: 0, self: 0, samples: 0 };
      cur.total += row.total;
      cur.self += row.self;
      cur.samples += row.samples;
      grouped.set(crate, cur);
    }
    return [...grouped.entries()].map(([name, v]) => ({ name, ...v }));
  }, [rows, mode]);

  const max = useMemo(() => (rows.reduce((m, row) => Math.max(m, sort === "total" ? row.total : row.self), 0) || 1), [rows, sort]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.summary}>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 8 }}>
        <button type="button" className={styles.buttonSmall} onClick={() => setMode("flat")}>
          {t("hot.flat")}
        </button>
        <button type="button" className={styles.buttonSmall} onClick={() => setMode("mods")}>
          {t("hot.mods")}
        </button>
        <button type="button" className={styles.buttonSmall} onClick={() => setSort((s) => (s === "total" ? "self" : "total"))}>
          {t("hot.sort")}: {sort}
        </button>
        <button type="button" className={styles.buttonSmall} onClick={() => setTopDown((v) => !v)}>
          {topDown ? t("hot.topDown") : t("hot.bottomUp")}
        </button>
        <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
          <option value={250}>250</option>
          <option value={100}>100</option>
        </select>
      </div>
      {mode === "flat" ? (
        <div className={styles.meterList}>
          <div className={styles.meterHead}>
            <span>Function</span>
            <span />
            <span>Self</span>
            <span>Total</span>
          </div>
          {ordered.slice(0, 250).map((row) => (
            <div
              key={row.id}
              className={styles.meterRow}
              title={`${row.full} (${row.id})`}
              onClick={() => setSelected(row)}
              style={{ cursor: "pointer" }}
            >
              <span className={styles.meterName}>{row.name}</span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span
                  className={styles.meterFill}
                  style={{ width: `${(((sort === "total" ? row.total : row.self) / max) * 100)}%` }}
                />
              </span>
              <span className={styles.meterValue}>{num(row.self, 2)}%</span>
              <span className={styles.meterExtra}>{num(row.total, 2)}%</span>
            </div>
          ))}
        </div>
      ) : (
        <div className={styles.meterList}>
          {modRows.map((row) => (
            <div key={row.name} className={styles.meterRow} title={row.name}>
              <span className={styles.meterName}>{row.name}</span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span
                  className={styles.meterFill}
                  style={{ width: `${(row.total / (modRows.reduce((m, r) => Math.max(m, r.total), 0) || 1)) * 100}%` }}
                />
              </span>
              <span className={styles.meterValue}>{num(row.self, 2)}</span>
              <span className={styles.meterExtra}>{num(row.total, 2)}</span>
            </div>
          ))}
        </div>
      )}
      {selected ? <FlameGraph rows={[selected]} /> : null}
    </div>
  );
}
