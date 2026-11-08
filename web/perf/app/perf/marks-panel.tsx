"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { MarkRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function MarksPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<MarkRow>(reader, capture, "marks");


  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <ul className={styles.meterList}>
      {rows.map((row) => (
        <li key={`${row.t}-${row.name}`} className={styles.meterRow}>
          <span className={styles.meterName}>{row.name}</span>
          <span />
          <span className={styles.meterValue}>{num(row.t, 1)} s</span>
          <span className={styles.meterExtra}>{row.side}</span>
        </li>
      ))}
    </ul>
  );
}
