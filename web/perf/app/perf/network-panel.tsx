"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped, num } from "@/lib/format";
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
  const packets = players.some((row) => row.packets_out + row.packets_in > 0);
  return (
    <div className={styles.metadata}>
      <p className={styles.cardNote}>{t("net.note")}</p>
      <dl className={styles.kv}>
        <div className={styles.kvRow}>
          <dt>{t("net.out")}</dt>
          <dd>{num(total.out_kib_s)} KiB/s</dd>
        </div>
        <div className={styles.kvRow}>
          <dt>{t("net.in")}</dt>
          <dd>{num(total.in_kib_s)} KiB/s</dd>
        </div>
        <div className={styles.kvRow}>
          <dt>{t("net.players")}</dt>
          <dd>{total.players}</dd>
        </div>
      </dl>
      {players.length > 0 ? (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("net.player")}</th>
              <th>{t("net.out")}</th>
              <th>{t("net.in")}</th>
              <th>{t("net.peakOut")}</th>
              {packets ? <th>{t("net.packetsOut")}</th> : null}
              {packets ? <th>{t("net.packetsIn")}</th> : null}
            </tr>
          </thead>
          <tbody>
            {players.map((row) => (
              <tr key={row.id}>
                <td>{t("net.playerN", { n: row.index })}</td>
                <td>{num(row.out_kib_s)} KiB/s</td>
                <td>{num(row.in_kib_s)} KiB/s</td>
                <td>{num(row.peak_out_kib_s)} KiB/s</td>
                {packets ? <td>{grouped(row.packets_out)}</td> : null}
                {packets ? <td>{grouped(row.packets_in)}</td> : null}
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
