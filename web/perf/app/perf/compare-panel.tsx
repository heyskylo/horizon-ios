"use client";

import { useRef } from "react";
import { changeMs, changePct, rowName, toneOf } from "@/lib/compare";
import { num } from "@/lib/format";
import { has, t } from "@/lib/i18n";
import type { DiffRow } from "@/lib/types";
import { useComparison } from "./comparison";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

const SHOWN = 200;

/** A health figure in words, any other row by its name. */
function metricName(row: DiffRow): string {
  const key = `compare.field.${row.id.replace(/^h:/, "")}`;
  return row.id.startsWith("h:") && has(key) ? t(key) : rowName(row);
}

/** Picks the second capture and lists what changed in the whole run: the health figures and the tick's spans. */
export function ComparePanel({ base }: { base: LoadedCapture }) {
  const { other, rows, warnings, error, ready, pick, clear } = useComparison();
  const picker = useRef<HTMLInputElement>(null);
  const table = rows.filter((row) => row.id.startsWith("h:") || row.id.startsWith("s:")).slice(0, SHOWN);
  const judged = table.some((row) => row.verdict !== "too_few_blocks");

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
      {ready && table.length > 0 && !judged ? <p className={styles.muted}>{t("compare.fewBlocks")}</p> : null}
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
              <th>{t("compare.a")}</th>
              <th>{t("compare.b")}</th>
              <th>{t("compare.change")}</th>
              {judged ? <th>{t("compare.verdict")}</th> : null}
            </tr>
          </thead>
          <tbody>
            {table.map((row) => (
              <tr key={row.id} title={row.id}>
                <td>{metricName(row)}</td>
                <td>{num(row.a, 2)}</td>
                <td>{num(row.b, 2)}</td>
                <td>{[changeMs(row), changePct(row)].filter(Boolean).join(" ") || "—"}</td>
                {judged ? (
                  <td className={styles.verdictCell} data-tone={toneOf(row.verdict)}>
                    {t(`compare.verdict.${row.verdict}`)}
                  </td>
                ) : null}
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
