"use client";

import { useMemo, useState } from "react";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import { mergeTree } from "@/lib/merge-tree";
import { buildNodes, entry, findNode } from "@/lib/tree-model";
import { groupScope } from "@/lib/tree-options";
import type { GroupKind } from "@/lib/tree-options";
import type { GroupRow, TreeRow } from "@/lib/types";
import { amount } from "@/lib/units";
import { FlameGraph } from "./flame-graph";
import { TREE_SHAPE, WHOLE_TREE, useTreeEnv } from "./tree-state";
import type { Scope, Target } from "./tree-state";
import { useRows } from "./use-capture";
import styles from "./page.module.css";
import tree from "./tree.module.css";

interface ScopeProps {
  scope: Scope;
  /** The rows of the whole tree, used when the scope has no filters of its own. */
  whole: readonly TreeRow[];
  focusKey?: string;
}

/** The flame graph of one set of samples, shaped as the list shows it: merged when merge mode is on, cut to the focused line. */
function FlameScope({ scope, whole, focusKey }: ScopeProps) {
  const env = useTreeEnv();
  const fetched = useRows<TreeRow>(env.reader, env.capture, "tree", { ...env.filters, ...scope.filters, ...TREE_SHAPE }, scope.filters !== null);
  const base = scope.filters === null ? whole : fetched.rows;
  const rows = useMemo(() => {
    const shown = env.merge ? mergeTree([...base]) : base;
    if (!focusKey) return shown;
    const node = findNode(buildNodes(shown, scope.id), focusKey);
    return node ? [entry(node)] : shown;
  }, [base, env.merge, focusKey, scope.id]);
  if (fetched.error) return <p className={styles.error}>{fetched.error}</p>;
  const reset = JSON.stringify([env.filters, scope.id, env.merge, focusKey ?? null]);
  return <FlameGraph key={reset} rows={rows} mods={env.mods} kind={env.kind} query={env.query} />;
}

/** A grouped tree has one flame graph per group: the reader picks which. */
function GroupFlame({ kind }: { kind: Exclude<GroupKind, "none"> }) {
  const env = useTreeEnv();
  const { rows, error } = useRows<GroupRow>(env.reader, env.capture, "group", { ...env.filters, by: kind, limit: 200 });
  const [picked, setPicked] = useState<string | null>(null);
  const choices = rows.filter((row) => row.key !== null);
  if (error) return <p className={styles.error}>{error}</p>;
  const current = choices.find((row) => row.key === picked) ?? choices[0];
  if (!current) return <p className={tree.none}>{t("tree.flame.empty")}</p>;
  const scope: Scope = { id: `G:${current.key ?? ""}`, filters: groupScope(kind, current) };
  return (
    <>
      <label className={`${tree.control} ${tree.flamePick}`}>
        <span className={tree.controlLabel}>{t(`tree.group.${kind}`)}</span>
        <select className={styles.select} value={current.key ?? ""} onChange={(event) => setPicked(event.target.value)}>
          {choices.map((row) => (
            <option key={row.key} value={row.key ?? ""}>
              {row.label} · {amount(row.value, env.kind)} · {num(row.pct, 1)}%
            </option>
          ))}
        </select>
      </label>
      <FlameScope scope={scope} whole={[]} />
    </>
  );
}

interface Props {
  whole: readonly TreeRow[];
  group: GroupKind;
  focused: Target | null;
}

/** The flame graph of what the call tree's controls select: its side, thread, selection, grouping and focus. */
export function TreeFlame({ whole, group, focused }: Props) {
  if (focused) return <FlameScope scope={focused.scope} whole={whole} focusKey={focused.key} />;
  if (group === "none") return <FlameScope scope={WHOLE_TREE} whole={whole} />;
  return <GroupFlame key={group} kind={group} />;
}
