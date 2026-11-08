"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { NetPlayerRow, NetTotalRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

export function NetworkPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<NetTotalRow | NetPlayerRow>(reader, capture, "net");

  if (error) return <p className={styles.error}>{error}</p>;
  const total = rows.find((row): row is NetTotalRow => "players" in row);
  const players = rows.filter((row): row is NetPlayerRow => "index" in row);
  if (!total) return <p className={styles.empty}>{t("meta.noNetwork")}</p>;
  return (
    <div className={styles.metadata}>
      <dl className={styles.kv}>
        <div className={styles.kvRow}>
          <dt>{t("summary.out")}</dt>
          <dd>{num(total.out_kib_s)} KiB/s</dd>
        </div>
        <div className={styles.kvRow}>
          <dt>{t("summary.in")}</dt>
          <dd>{num(total.in_kib_s)} KiB/s</dd>
        </div>
        <div className={styles.kvRow}>
          <dt>{t("health.players")}</dt>
          <dd>{total.players}</dd>
        </div>
      </dl>
      {players.length > 0 ? (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("summary.player")}</th>
              <th>{t("summary.out")}</th>
              <th>{t("summary.in")}</th>
              <th>{t("summary.peak")}</th>
            </tr>
          </thead>
          <tbody>
            {players.map((row) => (
              <tr key={row.id}>
                <td>{row.index}</td>
                <td>{num(row.out_kib_s)} KiB/s</td>
                <td>{num(row.in_kib_s)} KiB/s</td>
                <td>{num(row.peak_out_kib_s)} KiB/s</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
