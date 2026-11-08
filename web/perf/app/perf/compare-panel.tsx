"use client";

import { useRef, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DiffRow, Side } from "@/lib/types";
import { replyError } from "./use-capture";
import type { CaptureRef } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  base: CaptureRef;
  side: Side;
  other: CaptureRef | null;
  onPick: (file: File) => void;
}

const LIMIT = 200;

function change(row: DiffRow): string {
  if (row.diff === null) return "—";
  const sign = row.diff > 0 ? "+" : "";
  const pct = row.diff_pct === null ? "" : ` (${sign}${num(row.diff_pct, 0)}%)`;
  return `${sign}${num(row.diff, 2)} ms${pct}`;
}

const COLOR: Partial<Record<DiffRow["verdict"], string>> = { faster: "var(--success)", slower: "var(--danger)" };

export function ComparePanel({ reader, base, side, other, onPick }: Props) {
  const [rows, setRows] = useState<DiffRow[] | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  const run = () => {
    if (!other) return;
    setError(null);
    setRows(null);
    try {
      const reply = reader.diff(base.handle, other.handle, { side, limit: LIMIT });
      if (reply.ok === false || !reply.reply) {
        setError(replyError(reply.error));
        return;
      }
      setRows(reply.reply.rows);
      setWarnings(reply.reply.warnings.filter((warning) => !warning.includes("--")));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

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
            if (file) onPick(file);
            setRows(null);
          }}
        />
        <button type="button" className={styles.buttonSmall} onClick={() => picker.current?.click()}>
          {t("compare.choose")}
        </button>
        <button type="button" className={styles.button} onClick={run} disabled={!other}>
          {t("compare.run")}
        </button>
      </div>
      {error ? <p className={styles.error}>{error}</p> : null}
      <p className={styles.muted}>{other ? t("compare.pair", { a: base.name, b: other.name }) : t("compare.pick")}</p>
      {warnings.map((warning) => (
        <p key={warning} className={styles.muted}>
          {warning}
        </p>
      ))}
      {rows ? (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("compare.metric")}</th>
              <th>A</th>
              <th>B</th>
              <th>{t("compare.change")}</th>
              <th>{t("compare.verdict")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.id}>
                <td>{row.id.replace(/^[a-z]:/, "")}</td>
                <td>{num(row.a, 2)}</td>
                <td>{num(row.b, 2)}</td>
                <td>{change(row)}</td>
                <td style={{ color: COLOR[row.verdict] }}>{t(`compare.verdict.${row.verdict}`)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
