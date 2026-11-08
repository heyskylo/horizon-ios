"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num, grouped } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { MemoryRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function MemoryPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<MemoryRow>(reader, capture, "memory");

  if (error && !/memory snapshots/i.test(error)) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.noMemory")}</p>;
  return (
    <table className={styles.table}>
      <thead>
        <tr>
          <th>{t("meta.path")}</th>
          <th>{t("meta.value")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.id}>
            <td>{row.path}</td>
            <td>{row.unit === "mb" ? `${num(row.value, 1)} MiB` : grouped(row.value)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
