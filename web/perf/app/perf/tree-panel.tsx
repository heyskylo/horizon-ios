"use client";

import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { TreeRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import { Icons } from "./icons";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

const WRAPPERS = [
  "spans::time",
  "FnOnce::call_once",
];

function isWrapper(name: string): boolean {
  return WRAPPERS.some((w) => name.includes(w));
}

function filterMerge(rows: TreeRow[]): TreeRow[] {
  return rows
    .filter((row) => !isWrapper(row.name))
    .map((row) => ({ ...row, children: row.children ? filterMerge(row.children) : row.children }));
}

function Tree({
  rows,
  depth = 0,
  collapsed,
  toggle,
}: {
  rows: TreeRow[];
  depth?: number;
  collapsed: Set<string>;
  toggle: (id: string) => void;
}) {
  const max = rows.reduce((m, row) => Math.max(m, row.total), 0) || 1;
  return (
    <>
      {rows.map((row) => {
        const hasChildren = Boolean(row.children?.length);
        const isCollapsed = collapsed.has(row.id);
        return (
          <div
            key={row.id}
            style={{ "--depth": depth, paddingLeft: "calc(var(--depth, 0) * 0.75rem)" } as CSSProperties}
          >
            <div className={styles.meterRow} title={row.id}>
              <span className={styles.meterName} title={row.name}>
                {hasChildren ? (
                  <button
                    type="button"
                    onClick={() => toggle(row.id)}
                    style={{ border: 0, background: "transparent", color: "inherit", cursor: "pointer", marginRight: 4 }}
                  >
                    {isCollapsed ? <Icons.chevronRight /> : <Icons.chevronDown />}
                  </button>
                ) : (
                  <span style={{ display: "inline-block", width: 20 }} />
                )}
                {row.name}
              </span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span className={styles.meterFill} style={{ width: `${(row.total / max) * 100}%` }} />
              </span>
              <span className={styles.meterValue}>{num(row.total, 2)} ms</span>
              <span className={styles.meterExtra}>{num(row.pct, 1)}%</span>
            </div>
            {hasChildren && !isCollapsed && row.children ? (
              <Tree rows={row.children} depth={depth + 1} collapsed={collapsed} toggle={toggle} />
            ) : null}
          </div>
        );
      })}
    </>
  );
}

export function TreePanel({ reader, capture }: Props) {
  const [rows, setRows] = useState<TreeRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [merge, setMerge] = useState(false);

  useEffect(() => {
    setError(null);
    const reply = reader.query<TreeRow>(capture.handle, {
      op: "tree",
      file: capture.name,
      filters: { side: capture.side, limit: 200 },
    });
    if (!reply.ok) {
      setError(reply.error?.message ?? t("error.queryFailed"));
      setRows([]);
    } else setRows(reply.rows);
  }, [reader, capture]);

  const toggle = (id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const visibleRows = merge ? filterMerge(rows) : rows;

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.summary}>
      <button type="button" className={styles.buttonSmall} onClick={() => setMerge((v) => !v)}>
        {merge ? t("tree.mergeOn") : t("tree.mergeOff")}
      </button>
      <div className={styles.meterList}>
        <Tree rows={visibleRows} collapsed={collapsed} toggle={toggle} />
      </div>
    </div>
  );
}
