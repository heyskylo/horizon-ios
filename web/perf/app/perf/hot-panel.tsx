"use client";

import { useEffect, useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { HotRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function HotPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<HotRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<HotRow>(capture.handle, {
      op: "hot",
      file: capture.name,
      filters: { side: capture.side, limit: 40 },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  const max = useMemo(() => (rows.reduce((m, row) => Math.max(m, row.self), 0) || 1), [rows]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.meterList}>
      <div className={styles.meterHead}>
        <span>Function</span>
        <span />
        <span>Self</span>
        <span>Total</span>
      </div>
      {rows.map((row) => (
        <div key={row.id} className={styles.meterRow} title={row.full}>
          <span className={styles.meterName}>{row.name}</span>
          <span className={styles.meterTrack} aria-hidden="true">
            <span className={styles.meterFill} style={{ width: `${(row.self / max) * 100}%` }} />
          </span>
          <span className={styles.meterValue}>{num(row.self, 2)}%</span>
          <span className={styles.meterExtra}>{num(row.total, 2)}%</span>
        </div>
      ))}
    </div>
  );
}
