"use client";

import { useEffect, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { WarningRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function WarningsPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<WarningRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<WarningRow>(capture.handle, {
      op: "warnings",
      file: capture.name,
      filters: { side: capture.side },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <table className={styles.table}>
      <thead>
        <tr>
          <th>Time</th>
          <th>Code</th>
          <th>Text</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
          <tr key={`${row.t}-${index}`}>
            <td>{num(row.t, 1)} s</td>
            <td>{row.code}</td>
            <td>{row.text}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
