"use client";

import { Fragment, useMemo } from "react";
import type { CSSProperties } from "react";
import { crateGroup } from "@/lib/crate-group";
import type { CrateGroup } from "@/lib/crate-group";
import { grouped, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { mergeTree } from "@/lib/merge-tree";
import { buildNodes, entry, findNode, flatten, highlight, hottestPath, isOpen, last, search, toggled } from "@/lib/tree-model";
import type { FlatRow } from "@/lib/tree-model";
import type { TreeRow } from "@/lib/types";
import { amount } from "@/lib/units";
import { Icons } from "./icons";
import { Delta } from "./delta";
import { RowTools, infoText } from "./row-tools";
import { targetOf, useTreeEnv } from "./tree-state";
import type { Scope } from "./tree-state";
import styles from "./page.module.css";
import tree from "./tree.module.css";

export function Marked({ text, query }: { text: string; query: string }) {
  return (
    <>
      {highlight(text, query).map((piece, at) =>
        piece.hit ? (
          <mark key={at} className={tree.mark}>
            {piece.text}
          </mark>
        ) : (
          <Fragment key={at}>{piece.text}</Fragment>
        ),
      )}
    </>
  );
}

export function Swatch({ group }: { group: CrateGroup | null }) {
  const label = group ? t(`tree.crate.${group}`) : t("tree.crate.unknown");
  return <span className={tree.swatch} data-group={group ?? "unknown"} role="img" aria-label={label} title={label} />;
}

/** The column titles above every list of lines. */
export function TreeHeader() {
  const { unit, changes } = useTreeEnv();
  return (
    <div className={`${styles.meterHead} ${tree.row}`}>
      <span>{t("tree.function")}</span>
      <span className={tree.trackCell} />
      <span>{unit ? t(`tree.unit.${unit}`) : t("tree.unit.ms_per_tick")}</span>
      {changes ? <span>{t("compare.columnChange")}</span> : null}
      <span>{t("tree.share")}</span>
      <span>{t("tree.selfShare")}</span>
      <span className={tree.samplesCell}>{t("tree.samples")}</span>
    </div>
  );
}

function indent(depth: number): string {
  return `${Math.min(depth, 12)}`;
}

interface NodeLineProps {
  flat: Extract<FlatRow, { kind: "node" }>;
  scope: Scope;
  max: number;
}

function NodeLine({ flat, scope, max }: NodeLineProps) {
  const env = useTreeEnv();
  const { node, depth, open } = flat;
  const first = entry(node);
  const tail = last(node);
  const canOpen = node.children.length > 0 || node.folded !== null;
  const names = node.members.map((member) => member.name);
  const group = crateGroup(tail.crate, env.mods);
  const picked = env.selected?.scope.id === scope.id && env.selected.key === node.key;
  const explained = node.members.find((member) => infoText(member.function) !== null);
  return (
    <div className={`${styles.meterRow} ${tree.row}`} data-picked={picked} title={first.id}>
      <span className={`${styles.meterName} ${tree.name}`} style={{ "--depth": indent(depth) } as CSSProperties}>
        {canOpen ? (
          <button
            type="button"
            className={styles.disclosure}
            aria-expanded={open}
            aria-label={t(open ? "tree.collapse" : "tree.expand", { name: names.join(", ") })}
            onClick={() => env.setOpen(toggled(env.open, node.key, open))}
          >
            {open ? <Icons.chevronDown /> : <Icons.chevronRight />}
          </button>
        ) : (
          <span className={styles.disclosureSpace} />
        )}
        <Swatch group={group} />
        <button
          type="button"
          className={tree.nameButton}
          aria-pressed={picked}
          title={names.join(" > ")}
          onClick={() => env.select(picked ? null : targetOf(scope, node, env.merge))}
        >
          {node.members.map((member, at) => (
            <Fragment key={member.id}>
              {at > 0 ? (
                <span className={tree.chain} aria-hidden="true">
                  ›
                </span>
              ) : null}
              <Marked text={member.name} query={env.query} />
            </Fragment>
          ))}
        </button>
        <RowTools
          id={first.function}
          name={names.join(" › ")}
          about={explained?.function}
          subject={node.members.length > 1 ? explained?.name : undefined}
        />
      </span>
      <span className={`${styles.meterTrack} ${tree.trackCell}`} aria-hidden="true">
        <span className={styles.meterFill} style={{ width: `${Math.min(100, (first.total / max) * 100)}%` }} />
      </span>
      <span className={styles.meterValue}>{amount(first.total, env.kind)}</span>
      {env.changes ? <Delta row={env.changes.get(env.merge ? first.function : first.id)} /> : null}
      <span className={styles.meterExtra}>{num(first.pct, 1)}%</span>
      <span className={styles.meterExtra}>{tail.self_pct == null ? "—" : `${num(tail.self_pct, 1)}%`}</span>
      <span className={`${styles.meterExtra} ${tree.samplesCell}`}>{grouped(first.samples)}</span>
    </div>
  );
}

function FoldLine({ flat }: { flat: Extract<FlatRow, { kind: "fold" }> }) {
  const env = useTreeEnv();
  return (
    <div className={`${styles.meterRow} ${tree.row} ${tree.fold}`}>
      <span className={`${styles.meterName} ${tree.name}`} style={{ "--depth": indent(flat.depth) } as CSSProperties}>
        <span className={styles.disclosureSpace} />
        <span className={tree.swatchSpace} />
        {t("tree.smaller", { count: flat.folded.rows })}
      </span>
      <span className={tree.trackCell} />
      <span className={styles.meterValue}>{amount(flat.folded.total, env.kind)}</span>
      {env.changes ? <span /> : null}
      <span />
      <span />
      <span className={tree.samplesCell} />
    </div>
  );
}

interface BodyProps {
  rows: readonly TreeRow[];
  scope: Scope;
  /** Show only this line and what is below it. */
  focusKey?: string;
  /** How far the first line is indented: a group's lines sit under its header. */
  baseDepth?: number;
}

/** The lines of one set of rows: only the children of open lines are drawn. */
export function TreeBody({ rows, scope, focusKey, baseDepth = 0 }: BodyProps) {
  const env = useTreeEnv();
  const shown = useMemo(() => (env.merge ? mergeTree([...rows]) : rows), [env.merge, rows]);
  const nodes = useMemo(() => buildNodes(shown, scope.id), [shown, scope.id]);
  const roots = useMemo(() => {
    const focused = focusKey ? findNode(nodes, focusKey) : null;
    return focused ? [focused] : nodes;
  }, [nodes, focusKey]);
  const found = useMemo(() => search(roots, env.query), [roots, env.query]);
  const hot = useMemo(() => hottestPath(roots), [roots]);
  const flat = useMemo(
    () => flatten(roots, (node) => isOpen(env.open, node.key, hot.has(node.key), found.parents.has(node.key)), baseDepth),
    [roots, env.open, hot, found, baseDepth],
  );
  const max = roots.reduce((top, node) => Math.max(top, entry(node).total), 0) || 1;
  return (
    <>
      {env.query.trim() !== "" && found.hits.size === 0 ? <p className={tree.none}>{t("tree.noMatch")}</p> : null}
      {flat.map((row) =>
        row.kind === "node" ? <NodeLine key={row.node.key} flat={row} scope={scope} max={max} /> : <FoldLine key={row.key} flat={row} />,
      )}
    </>
  );
}
