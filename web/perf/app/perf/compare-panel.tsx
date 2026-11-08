"use client";

import { useRef } from "react";
import { changeMs, changePct, rowName, toneOf } from "@/lib/compare";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { useComparison } from "./comparison";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

const SHOWN = 200;

/** Picks the second capture and lists what changed in the whole run: the health figures and the tick's spans. */
export function ComparePanel({ base }: { base: LoadedCapture }) {
  const { other, rows, warnings, error, ready, pick, clear } = useComparison();
  const picker = useRef<HTMLInputElement>(null);
  const table = rows.filter((row) => row.id.startsWith("h:") || row.id.startsWith("s:")).slice(0, SHOWN);

  return (
    <div className={styles.metadata}>
      <div className={styles.actions}>
        <input
          ref={picker}
          type="file"
          accept=".hzperf"
          className={styles.fileInput}
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void pick(file);
            event.target.value = "";
          }}
        />
        <button type="button" className={other ? styles.buttonSmall : styles.button} onClick={() => picker.current?.click()}>
          {other ? t("compare.another") : t("compare.choose")}
        </button>
        {other ? (
          <button type="button" className={styles.buttonSmall} onClick={clear}>
            {t("compare.stop")}
          </button>
        ) : null}
      </div>
      {error ? <p className={styles.error}>{error}</p> : null}
      <p className={styles.muted}>{other ? t("compare.pair", { a: base.name, b: other.name }) : t("compare.pick")}</p>
      {other && ready ? <p className={styles.muted}>{t("compare.elsewhere")}</p> : null}
      {warnings.map((warning) => (
        <p key={warning} className={styles.muted}>
          {warning}
        </p>
      ))}
      {ready && table.length > 0 ? (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("compare.metric")}</th>
              <th>{t("compare.columnA")}</th>
              <th>{t("compare.columnB")}</th>
              <th>{t("compare.change")}</th>
              <th>{t("compare.verdict")}</th>
            </tr>
          </thead>
          <tbody>
            {table.map((row) => (
              <tr key={row.id} title={row.id}>
                <td>{rowName(row)}</td>
                <td>{num(row.a, 2)}</td>
                <td>{num(row.b, 2)}</td>
                <td>{[changeMs(row), changePct(row)].filter(Boolean).join(" ") || "—"}</td>
                <td className={styles.verdictCell} data-tone={toneOf(row.verdict)}>
                  {t(`compare.verdict.${row.verdict}`)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
