"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface GpuPass {
  id: string;
  name: string;
  source?: string;
  ms?: number | null;
  share?: number | null;
}

interface GpuDevice {
  id: string;
  adapter?: string;
  backend?: string;
  driver?: string;
}

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function GpuPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<GpuDevice | GpuPass>(reader, capture, "gpu");
  const device = rows.find((row): row is GpuDevice => "adapter" in row);
  const passes = rows.filter((row): row is GpuPass => "name" in row);

  if (error) return <p className={styles.error}>{error}</p>;
  return (
    <div className={styles.metadata}>
      {device ? (
        <dl className={styles.kv}>
          {[
            [t("meta.gpu"), device.adapter],
            [t("gpu.backend"), device.backend],
            [t("gpu.driver"), device.driver],
          ].map(([label, value]) => (
            <div key={label} className={styles.kvRow}>
              <dt>{label}</dt>
              <dd>{value ?? t("meta.none")}</dd>
            </div>
          ))}
        </dl>
      ) : null}
      {passes.length === 0 ? (
        <p className={styles.empty}>{t("meta.empty")}</p>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("table.pass")}</th>
              <th>{t("gpu.ms")}</th>
              <th>{t("table.share")}</th>
            </tr>
          </thead>
          <tbody>
            {passes.map((row) => (
              <tr key={row.id}>
                <td title={row.source}>{row.name}</td>
                <td>{num(row.ms, 3)}</td>
                <td>{num(row.share == null ? null : row.share * 100, 1)}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
