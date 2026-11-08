"use client";

import { changeMs, changePct, onlyInComparison, rowName, toneOf } from "@/lib/compare";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { DiffRow } from "@/lib/types";
import { RowTools } from "./row-tools";
import styles from "./page.module.css";

const NEW_ROWS = 12;

/** What the verdict says in a few letters: significant ones in words, the quiet ones as a mark. */
function tag(verdict: DiffRow["verdict"]): string {
  switch (verdict) {
    case "within_noise":
    case "negligible":
      return "~";
    case "too_few_blocks":
      return "?";
    default:
      return t(`compare.verdict.${verdict}`);
  }
}

/** The change of one row, coloured by its verdict; a row the diff has no answer for keeps an empty cell so the columns stay lined up. */
export function Delta({ row }: { row: DiffRow | undefined }) {
  if (!row) return <span className={styles.delta} />;
  const tone = toneOf(row.verdict);
  const change = changeMs(row);
  const verdict = t(`compare.verdict.${row.verdict}`);
  const title = [verdict, change, changePct(row)].filter(Boolean).join(" · ");
  return (
    <span className={styles.delta} data-tone={tone} title={title}>
      {change === null ? null : <span className={styles.deltaChange}>{change}</span>}
      {change === null ? null : <span className={styles.deltaPct}>{changePct(row)}</span>}
      <span className={styles.deltaTag}>{tag(row.verdict)}</span>
    </span>
  );
}

interface NewProps {
  rows: readonly DiffRow[];
  prefix: "s:" | "f:";
  /** The ids the view already draws. */
  shown: ReadonlySet<string>;
}

/** Rows only the comparison has: a view cannot place them among its own, so it lists them apart. */
export function NewRows({ rows, prefix, shown }: NewProps) {
  const fresh = onlyInComparison(rows, prefix, shown, NEW_ROWS);
  if (fresh.length === 0) return null;
  return (
    <section className={styles.onlyThere}>
      <h3 className={styles.onlyTitle}>{t("compare.onlyThere")}</h3>
      <div className={styles.meterList}>
        {fresh.map((row) => (
          <div key={row.id} className={styles.meterOnly}>
            <span className={styles.meterName} title={rowName(row)}>
              <span className={styles.nameText}>{rowName(row)}</span>
              <RowTools id={row.id} name={rowName(row)} />
            </span>
            <span className={styles.meterValue}>{num(row.b, 2)} ms</span>
            <Delta row={row} />
          </div>
        ))}
      </div>
    </section>
  );
}
