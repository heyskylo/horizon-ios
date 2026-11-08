"use client";

import type { CSSProperties } from "react";
import { grouped, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { isOpen, toggled } from "@/lib/tree-model";
import { groupScope } from "@/lib/tree-options";
import type { GroupKind } from "@/lib/tree-options";
import type { GroupRow, TreeRow } from "@/lib/types";
import { amount } from "@/lib/units";
import { Icons } from "./icons";
import { RowTools } from "./row-tools";
import { Marked, TreeBody } from "./tree-rows";
import { TREE_SHAPE, useTreeEnv } from "./tree-state";
import type { Scope } from "./tree-state";
import { useRows } from "./use-capture";
import styles from "./page.module.css";
import tree from "./tree.module.css";

/** A call tree of the samples a scope keeps: the given rows, or the ones its own query answers with. */
export function ScopedTree({ scope, focusKey, rows, baseDepth }: { scope: Scope; focusKey?: string; rows: readonly TreeRow[] | null; baseDepth?: number }) {
  const env = useTreeEnv();
  const fetched = useRows<TreeRow>(env.reader, env.capture, "tree", { ...env.filters, ...scope.filters, ...TREE_SHAPE }, rows === null);
  if (fetched.error) return <p className={styles.error}>{fetched.error}</p>;
  return <TreeBody rows={rows ?? fetched.rows} scope={scope} focusKey={focusKey} baseDepth={baseDepth} />;
}

/** The samples divided by one context; opening a group draws the call tree of just its samples. */
export function GroupList({ kind }: { kind: Exclude<GroupKind, "none"> }) {
  const env = useTreeEnv();
  const { rows, error } = useRows<GroupRow>(env.reader, env.capture, "group", { ...env.filters, by: kind, limit: 200 });
  if (error) return <p className={styles.error}>{error}</p>;
  const max = rows.reduce((top, row) => Math.max(top, row.value), 0) || 1;
  const biggest = rows.findIndex((row) => row.key !== null);
  return (
    <>
      {rows.map((row, at) => {
        const scope: Scope = { id: `G:${row.key ?? ""}`, filters: groupScope(kind, row) };
        const label = row.key === null ? t("tree.noContext") : row.label;
        const open = scope.filters !== null && isOpen(env.open, scope.id, at === biggest, false);
        return (
          <div key={scope.id}>
            <div className={`${styles.meterRow} ${tree.row} ${tree.group}`}>
              <span className={`${styles.meterName} ${tree.name}`} style={{ "--depth": "0" } as CSSProperties}>
                {scope.filters ? (
                  <button
                    type="button"
                    className={styles.disclosure}
                    aria-expanded={open}
                    aria-label={t(open ? "tree.collapse" : "tree.expand", { name: label })}
                    onClick={() => env.setOpen(toggled(env.open, scope.id, open))}
                  >
                    {open ? <Icons.chevronDown /> : <Icons.chevronRight />}
                  </button>
                ) : (
                  <span className={styles.disclosureSpace} />
                )}
                <span className={tree.groupLabel} title={label}>
                  <Marked text={label} query={env.query} />
                </span>
                <RowTools id={row.key} name={label} />
              </span>
              <span className={`${styles.meterTrack} ${tree.trackCell}`} aria-hidden="true">
                <span className={styles.meterFill} style={{ width: `${(row.value / max) * 100}%` }} />
              </span>
              <span className={styles.meterValue}>{amount(row.value, env.kind)}</span>
              <span className={styles.meterExtra}>{num(row.pct, 1)}%</span>
              <span />
              <span className={`${styles.meterExtra} ${tree.samplesCell}`}>{grouped(row.samples)}</span>
            </div>
            {open ? <ScopedTree scope={scope} rows={null} baseDepth={1} /> : null}
          </div>
        );
      })}
    </>
  );
}
