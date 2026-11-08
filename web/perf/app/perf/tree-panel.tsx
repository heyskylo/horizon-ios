"use client";

import { useCallback, useMemo, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { groupsIn, modCrates } from "@/lib/crate-group";
import { t } from "@/lib/i18n";
import { ALL_CLOSED, HOT_OPEN } from "@/lib/tree-model";
import type { OpenState } from "@/lib/tree-model";
import { sampleFilters } from "@/lib/tree-options";
import type { GroupKind, SampleOptions, SampleSelection, ViewKind } from "@/lib/tree-options";
import type { TreeRow } from "@/lib/types";
import { amountKind } from "@/lib/units";
import { useChanges } from "./comparison";
import { TreeFlame } from "./tree-flame";
import { GroupList, ScopedTree } from "./tree-groups";
import { TreeDetail } from "./tree-detail";
import { TreeBody, TreeHeader } from "./tree-rows";
import { TREE_SHAPE, TreeEnvProvider, WHOLE_TREE, useSampleOptions } from "./tree-state";
import type { Target, TreeEnv } from "./tree-state";
import { TreeLegend, TreeToolbar } from "./tree-toolbar";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";
import tree from "./tree.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

interface ViewProps extends Props {
  options: SampleOptions;
}

function TreeView({ reader, capture, options }: ViewProps) {
  const [selection, setSelection] = useState<SampleSelection>(options.selections[0]);
  const [group, setGroup] = useState<GroupKind>("none");
  const [thread, setThread] = useState<string | null>(null);
  const [view, setView] = useState<ViewKind>("list");
  const [merge, setMerge] = useState(false);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<OpenState>(HOT_OPEN);
  const [selected, select] = useState<Target | null>(null);
  const [focused, setFocused] = useState<Target | null>(null);

  // Grouping by thread shows every thread, so a thread picked earlier must not narrow it.
  const only = group === "thread" ? null : thread;
  const filters = useMemo(() => sampleFilters(selection, only), [selection, only]);
  const meta = capture.meta.rows.find((row) => row.side === capture.side);
  const mods = useMemo(() => modCrates(meta?.mods ?? []), [meta]);
  const whole = useRows<TreeRow>(reader, capture, "tree", { ...filters, ...TREE_SHAPE });
  const compare = useChanges();
  // The diff measures every tick of the main thread, so only that tree has changes to show.
  const changes = compare && selection === "all" && thread === null && group === "none" ? compare.changes : null;

  const fresh = useCallback(() => {
    setOpen(HOT_OPEN);
    select(null);
    setFocused(null);
  }, []);

  const env: TreeEnv = {
    reader,
    capture,
    filters,
    merge,
    query,
    mods,
    kind: amountKind(whole.reply?.unit),
    unit: whole.reply?.unit,
    changes,
    open,
    setOpen,
    selected,
    select,
    focus: setFocused,
  };

  if (whole.error) return <p className={styles.error}>{whole.error}</p>;
  return (
    <TreeEnvProvider value={env}>
      <TreeToolbar
        options={options}
        side={capture.side}
        selection={selection}
        group={group}
        thread={thread}
        view={view}
        merge={merge}
        query={query}
        onSelection={(value) => {
          setSelection(value);
          fresh();
        }}
        onGroup={(value) => {
          setGroup(value);
          fresh();
        }}
        onThread={(value) => {
          setThread(value);
          fresh();
        }}
        onView={setView}
        onMerge={() => {
          setMerge((value) => !value);
          fresh();
        }}
        onQuery={setQuery}
        onCollapse={() => setOpen(ALL_CLOSED)}
        onHottest={() => setOpen(HOT_OPEN)}
      />
      <TreeLegend groups={groupsIn(whole.rows, mods)} />
      {focused ? (
        <div className={tree.focusBar}>
          <span className={tree.focusName}>{t("tree.focused", { name: focused.names.join(" › ") })}</span>
          <button type="button" className={styles.buttonSmall} onClick={() => setFocused(null)}>
            {t("tree.showAll")}
          </button>
        </div>
      ) : null}
      {view === "flame" ? (
        <TreeFlame whole={whole.rows} group={group} focused={focused} />
      ) : (
        <div className={tree.layout} data-detail={selected !== null}>
          <div className={tree.list} data-delta={changes !== null}>
            <TreeHeader />
            {focused ? (
              <ScopedTree scope={focused.scope} focusKey={focused.key} rows={focused.scope.filters ? null : whole.rows} />
            ) : group === "none" ? (
              <TreeBody rows={whole.rows} scope={WHOLE_TREE} />
            ) : (
              <GroupList kind={group} />
            )}
          </div>
          {selected ? (
            <TreeDetail
              key={`${selected.scope.id}${selected.key}`}
              target={selected}
              focused={focused?.key === selected.key}
              onFocus={() => setFocused(selected)}
              onClose={() => select(null)}
            />
          ) : null}
        </div>
      )}
    </TreeEnvProvider>
  );
}

export function TreePanel({ reader, capture }: Props) {
  const options = useSampleOptions(reader, capture);
  if (options.selections.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return <TreeView key={`${capture.handle}:${capture.side}`} reader={reader} capture={capture} options={options} />;
}
