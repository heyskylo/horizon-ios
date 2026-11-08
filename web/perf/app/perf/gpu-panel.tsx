"use client";

import { useEffect, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
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
  counters?: boolean;
}

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

async function copyText(value: string) {
  try {
    await navigator.clipboard.writeText(value);
  } catch {
    // ignore
  }
}

export function GpuPanel({ reader, capture }: Props) {
  const [device, setDevice] = useState<GpuDevice | null>(null);
  const [passes, setPasses] = useState<GpuPass[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    try {
      const deviceReply = reader.query<GpuDevice>(capture.handle, {
        op: "gpu",
        file: capture.name,
        filters: { side: capture.side, kind: "device" } as Record<string, unknown>,
      });
      // The gpu op returns device facts and passes together in most builds; tolerate shapes.
      if (deviceReply.ok && deviceReply.rows.length > 0 && "adapter" in (deviceReply.rows[0] as unknown as Record<string, unknown>)) {
        setDevice(deviceReply.rows[0] as GpuDevice);
      }
      const passReply = reader.query<GpuPass>(capture.handle, {
        op: "gpu",
        file: capture.name,
        filters: { side: capture.side },
      });
      if (!passReply.ok) {
        setError(passReply.error?.message ?? t("error.queryFailed"));
        setPasses([]);
      } else {
        const rows = passReply.rows as unknown as Array<Record<string, unknown>>;
        setPasses(
          rows
            .filter((row) => "name" in row)
            .map((row) => ({
              id: String(row.id ?? ""),
              name: String(row.name ?? ""),
              source: row.source ? String(row.source) : undefined,
              ms: typeof row.ms === "number" ? row.ms : null,
              share: typeof row.share === "number" ? row.share : null,
            })),
        );
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  }, [reader, capture]);

  if (error) return <p className={styles.error}>{error}</p>;
  return (
    <div className={styles.metadata}>
      {device ? (
        <dl className={styles.kv}>
          <div className={styles.kvRow}>
            <dt>{t("meta.gpu")}</dt>
            <dd>{device.adapter ?? t("meta.none")}</dd>
          </div>
          <div className={styles.kvRow}>
            <dt>backend</dt>
            <dd>{device.backend ?? t("meta.none")}</dd>
          </div>
          <div className={styles.kvRow}>
            <dt>driver</dt>
            <dd>{device.driver ?? t("meta.none")}</dd>
          </div>
        </dl>
      ) : null}
      {passes.length === 0 ? (
        <p className={styles.empty}>{t("meta.empty")}</p>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>Pass</th>
              <th>GPU ms</th>
              <th>Share</th>
              <th>Id</th>
            </tr>
          </thead>
          <tbody>
            {passes.map((row) => (
              <tr key={row.id}>
                <td title={row.source}>{row.name}</td>
                <td>{num(row.ms, 3)}</td>
                <td>{num(row.share == null ? null : row.share * 100, 1)}%</td>
                <td>
                  <button type="button" className={styles.buttonSmall} onClick={() => void copyText(row.id)}>
                    {t("copy.id", { id: row.id })}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
