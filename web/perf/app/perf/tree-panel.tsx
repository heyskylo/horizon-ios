"use client";

import { useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { mergeTree } from "@/lib/merge-tree";
import type { TreeRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import { Icons } from "./icons";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

function Tree({
  rows,
  depth = 0,
  max,
  collapsed,
  startCollapsed,
  toggle,
}: {
  rows: TreeRow[];
  depth?: number;
  max: number;
  collapsed: Set<string>;
  startCollapsed: boolean;
  toggle: (id: string) => void;
}) {
  return (
    <>
      {rows.map((row) => {
        const hasChildren = Boolean(row.children?.length);
        const isCollapsed = collapsed.has(row.id) !== startCollapsed;
        return (
          <div key={row.id}>
            <div className={styles.meterRow} title={row.id}>
              <span className={styles.meterName} title={row.name} style={{ paddingLeft: depth * 14 }}>
                {hasChildren ? (
                  <button
                    type="button"
                    className={styles.disclosure}
                    aria-expanded={!isCollapsed}
                    aria-label={row.name}
                    onClick={() => toggle(row.id)}
                  >
                    {isCollapsed ? <Icons.chevronRight /> : <Icons.chevronDown />}
                  </button>
                ) : (
                  <span className={styles.disclosureSpace} />
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
              <Tree rows={row.children} depth={depth + 1} max={max} collapsed={collapsed} startCollapsed={startCollapsed} toggle={toggle} />
            ) : null}
          </div>
        );
      })}
    </>
  );
}

export function TreePanel({ reader, capture }: Props) {
  const { rows, error } = useRows<TreeRow>(reader, capture, "tree", { limit: 200 });
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [merge, setMerge] = useState(false);


  const toggle = (id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const visibleRows = useMemo(() => (merge ? mergeTree(rows) : rows), [merge, rows]);

  const treeMax = visibleRows.reduce((m, row) => Math.max(m, row.total), 0) || 1;

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div>
      <div className={styles.toolbar}>
        <button
          type="button"
          className={styles.buttonSmall}
          aria-pressed={merge}
          onClick={() => {
            setMerge((v) => !v);
            setCollapsed(new Set());
          }}
        >
          {t("tree.merge")}
        </button>
      </div>
      <div className={styles.meterList}>
        <Tree rows={visibleRows} max={treeMax} collapsed={collapsed} startCollapsed={merge} toggle={toggle} />
      </div>
    </div>
  );
}
