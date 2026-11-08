"use client";

import { useEffect, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DimensionRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function WorldPanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<DimensionRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    const reply = reader.query<DimensionRow>(capture.handle, {
      op: "world",
      file: capture.name,
      filters: { side: capture.side },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.noWorld")}</p>;
  return (
    <table className={styles.table}>
      <thead>
        <tr>
          <th>{t("meta.dimension")}</th>
          <th>{t("meta.chunks")}</th>
          <th>{t("meta.entities")}</th>
          <th>{t("meta.blockEntities")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.id}>
            <td>{row.name}</td>
            <td>{grouped(row.chunks)}</td>
            <td>{grouped(row.entities)}</td>
            <td>{grouped(row.block_entities)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
