"use client";

import { CRATE_GROUPS } from "@/lib/crate-group";
import type { CrateGroup } from "@/lib/crate-group";
import { t } from "@/lib/i18n";
import type { GroupKind, SampleOptions, SampleSelection } from "@/lib/tree-options";
import type { Side } from "@/lib/types";
import { Swatch } from "./tree-rows";
import styles from "./page.module.css";
import tree from "./tree.module.css";

interface Props {
  options: SampleOptions;
  side: Side;
  selection: SampleSelection;
  group: GroupKind;
  thread: string | null;
  merge: boolean;
  query: string;
  onSelection: (value: SampleSelection) => void;
  onGroup: (value: GroupKind) => void;
  onThread: (value: string) => void;
  onMerge: () => void;
  onQuery: (value: string) => void;
  onCollapse: () => void;
  onHottest: () => void;
}

export function TreeToolbar(props: Props) {
  const { options, side, selection, group, merge, query } = props;
  const pace = side === "client" ? "Frames" : "Ticks";
  const thread = props.thread ?? options.thread ?? "";
  return (
    <div className={tree.controls}>
      <label className={tree.control}>
        <span className={tree.controlLabel}>{t("tree.selection")}</span>
        <select
          className={styles.select}
          value={selection}
          onChange={(event) => {
            const next = options.selections.find((id) => id === event.target.value);
            if (next) props.onSelection(next);
          }}
        >
          {options.selections.map((id) => (
            <option key={id} value={id}>
              {t(`tree.selection.${id}${id === "all" || id === "slow" ? pace : ""}`)}
            </option>
          ))}
        </select>
      </label>
      {options.groups.length > 1 ? (
        <label className={tree.control}>
          <span className={tree.controlLabel}>{t("tree.groupBy")}</span>
          <select
            className={styles.select}
            value={group}
            onChange={(event) => {
              const next = options.groups.find((id) => id === event.target.value);
              if (next) props.onGroup(next);
            }}
          >
            {options.groups.map((id) => (
              <option key={id} value={id}>
                {t(`tree.group.${id}`)}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      {options.threads.length > 1 && group !== "thread" ? (
        <label className={tree.control}>
          <span className={tree.controlLabel}>{t("tree.thread")}</span>
          <select className={styles.select} value={thread} onChange={(event) => props.onThread(event.target.value)}>
            {options.threads.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      <label className={`${tree.control} ${tree.searchControl}`}>
        <span className={tree.controlLabel}>{t("tree.search")}</span>
        <input
          type="search"
          className={tree.search}
          value={query}
          placeholder={t("tree.searchHint")}
          onChange={(event) => props.onQuery(event.target.value)}
        />
      </label>
      <div className={tree.buttons}>
        <button type="button" className={styles.buttonSmall} aria-pressed={merge} onClick={props.onMerge}>
          {t("tree.merge")}
        </button>
        <button type="button" className={styles.buttonSmall} onClick={props.onCollapse}>
          {t("tree.collapseAll")}
        </button>
        <button type="button" className={styles.buttonSmall} onClick={props.onHottest}>
          {t("tree.hottest")}
        </button>
      </div>
    </div>
  );
}

/** What the row colours mean, for the groups the tree has. */
export function TreeLegend({ groups }: { groups: readonly CrateGroup[] }) {
  if (groups.length === 0) return null;
  return (
    <ul className={tree.legend} aria-label={t("tree.legend")}>
      {CRATE_GROUPS.filter((group) => groups.includes(group)).map((group) => (
        <li key={group} className={tree.legendItem}>
          <Swatch group={group} />
          {t(`tree.crate.${group}`)}
        </li>
      ))}
    </ul>
  );
}
