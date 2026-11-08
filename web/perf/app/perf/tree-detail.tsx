"use client";

import { Fragment, useMemo, useState } from "react";
import { crateGroup } from "@/lib/crate-group";
import { grouped, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { buildNodes, flatten } from "@/lib/tree-model";
import { CONTEXT_KEYS } from "@/lib/tree-options";
import type { ContextKey } from "@/lib/tree-options";
import type { GroupRow, TreeRow } from "@/lib/types";
import { amount } from "@/lib/units";
import { Icons } from "./icons";
import { Swatch } from "./tree-rows";
import { useTreeEnv } from "./tree-state";
import type { Target } from "./tree-state";
import { useRows } from "./use-capture";
import styles from "./page.module.css";
import tree from "./tree.module.css";

const SPLIT_LIMIT = 10;
const CALLERS_LIMIT = 8;
const CALLERS_DEPTH = 3;

function SplitList({ target, by }: { target: Target; by: ContextKey }) {
  const env = useTreeEnv();
  const { rows } = useRows<GroupRow>(
    env.reader,
    env.capture,
    "split",
    { ...env.filters, ...target.scope.filters, by, limit: SPLIT_LIMIT },
    true,
    [target.id],
  );
  const named = rows.some((row) => row.key !== null);
  if (!named) return null;
  const max = rows.reduce((top, row) => Math.max(top, row.value), 0) || 1;
  return (
    <section className={tree.detailBlock}>
      <h4 className={tree.detailHeading}>{t(`tree.split.${by}`)}</h4>
      <div className={styles.meterList}>
        {rows.map((row) => {
          const label = row.key === null ? t("tree.noContext") : row.label;
          return (
            <div key={row.key ?? ""} className={`${styles.meterRow} ${tree.detailRow}`} title={label}>
              <span className={styles.meterName}>{label}</span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span className={styles.meterFill} style={{ width: `${(row.value / max) * 100}%` }} />
              </span>
              <span className={styles.meterValue}>{amount(row.value, env.kind)}</span>
              <span className={styles.meterExtra}>{num(row.pct, 1)}%</span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function CallerList({ target }: { target: Target }) {
  const env = useTreeEnv();
  const { rows, error } = useRows<TreeRow>(
    env.reader,
    env.capture,
    "callers",
    { ...env.filters, ...target.scope.filters, depth: CALLERS_DEPTH, limit: CALLERS_LIMIT },
    true,
    [target.function],
  );
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const nodes = useMemo(() => buildNodes(rows, "callers"), [rows]);
  const flat = useMemo(() => flatten(nodes, (node) => open.has(node.key)), [nodes, open]);
  const max = nodes.reduce((top, node) => Math.max(top, node.members[0].total), 0) || 1;
  if (error) return <p className={tree.none}>{error}</p>;
  return (
    <section className={tree.detailBlock}>
      <h4 className={tree.detailHeading}>{t("tree.callers")}</h4>
      {nodes.length === 0 ? <p className={tree.none}>{t("tree.noCallers")}</p> : null}
      <div className={styles.meterList}>
        {flat.map((line) => {
          if (line.kind === "fold") {
            return (
              <div key={line.key} className={`${styles.meterRow} ${tree.detailRow} ${tree.fold}`}>
                <span className={styles.meterName} style={{ paddingLeft: line.depth * 14 }}>
                  <span className={styles.disclosureSpace} />
                  {t("tree.smaller", { count: line.folded.rows })}
                </span>
                <span />
                <span className={styles.meterValue}>{amount(line.folded.total, env.kind)}</span>
                <span />
              </div>
            );
          }
          const { node } = line;
          const row = node.members[0];
          const canOpen = node.children.length > 0;
          return (
            <div key={node.key} className={`${styles.meterRow} ${tree.detailRow}`} title={row.name}>
              <span className={styles.meterName} style={{ paddingLeft: line.depth * 14 }}>
                {canOpen ? (
                  <button
                    type="button"
                    className={styles.disclosure}
                    aria-expanded={line.open}
                    aria-label={t(line.open ? "tree.collapse" : "tree.expand", { name: row.name })}
                    onClick={() =>
                      setOpen((current) => {
                        const next = new Set(current);
                        if (next.has(node.key)) next.delete(node.key);
                        else next.add(node.key);
                        return next;
                      })
                    }
                  >
                    {line.open ? <Icons.chevronDown /> : <Icons.chevronRight />}
                  </button>
                ) : (
                  <span className={styles.disclosureSpace} />
                )}
                <span className={tree.groupLabel}>{row.name}</span>
              </span>
              <span className={styles.meterTrack} aria-hidden="true">
                <span className={styles.meterFill} style={{ width: `${(row.total / max) * 100}%` }} />
              </span>
              <span className={styles.meterValue}>{amount(row.total, env.kind)}</span>
              <span className={styles.meterExtra}>{num(row.pct, 1)}%</span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

interface Props {
  target: Target;
  focused: boolean;
  onFocus: () => void;
  onClose: () => void;
}

/** What the selected line costs: its numbers, the contexts it spends them in, and who calls it. */
export function TreeDetail({ target, focused, onFocus, onClose }: Props) {
  const env = useTreeEnv();
  const group = crateGroup(target.last.crate, env.mods);
  return (
    <aside className={tree.detail} aria-label={t("tree.detail")}>
      <header className={tree.detailHead}>
        <h3 className={tree.detailTitle}>
          {target.names.map((name, at) => (
            <Fragment key={at}>
              {at > 0 ? (
                <span className={tree.chain} aria-hidden="true">
                  ›
                </span>
              ) : null}
              {name}
            </Fragment>
          ))}
        </h3>
        <button type="button" className={styles.buttonSmall} onClick={onClose}>
          {t("tree.close")}
        </button>
      </header>
      <dl className={tree.facts}>
        <div>
          <dt>{t("tree.total")}</dt>
          <dd>{amount(target.entry.total, env.kind)}</dd>
        </div>
        <div>
          <dt>{t("tree.share")}</dt>
          <dd>{num(target.entry.pct, 1)}%</dd>
        </div>
        <div>
          <dt>{t("tree.own")}</dt>
          <dd>{target.last.self == null ? "—" : amount(target.last.self, env.kind)}</dd>
        </div>
        <div>
          <dt>{t("tree.selfShare")}</dt>
          <dd>{target.last.self_pct == null ? "—" : `${num(target.last.self_pct, 1)}%`}</dd>
        </div>
        <div>
          <dt>{t("tree.samples")}</dt>
          <dd>{grouped(target.entry.samples)}</dd>
        </div>
        {target.last.crate ? (
          <div>
            <dt>{t("tree.crate")}</dt>
            <dd className={tree.crateName}>
              <Swatch group={group} />
              {target.last.crate}
            </dd>
          </div>
        ) : null}
      </dl>
      {focused ? null : (
        <button type="button" className={`${styles.buttonSmall} ${tree.buttonFocus}`} onClick={onFocus}>
          {t("tree.focus")}
        </button>
      )}
      {CONTEXT_KEYS.map((by) => (
        <SplitList key={by} target={target} by={by} />
      ))}
      <CallerList target={target} />
    </aside>
  );
}
