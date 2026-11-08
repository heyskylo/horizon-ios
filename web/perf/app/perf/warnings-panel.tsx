"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { WarningRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function WarningsPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<WarningRow>(reader, capture, "warnings");


  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <table className={styles.table}>
      <thead>
        <tr>
          <th>{t("table.time")}</th>
          <th>{t("table.warning")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
          <tr key={`${row.t}-${index}`}>
            <td>{num(row.t, 1)} s</td>
            <td>{t(`${row.code}`, { seconds: num(row.args[0] ?? 0, 0), mib: num((row.args[1] ?? 0) / (1024 * 1024), 1) })}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
