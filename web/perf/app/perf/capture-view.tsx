"use client";

import { createElement, useEffect, useMemo, useRef, useState } from "react";
import type { ComponentType } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { t } from "@/lib/i18n";
import { tabsFor } from "@/lib/tabs";
import type { Tab } from "@/lib/tabs";
import type { NetPlayerRow, NetTotalRow, Side } from "@/lib/types";
import { GpuPanel } from "./gpu-panel";
import { ComparePanel } from "./compare-panel";
import { MetadataPanel } from "./metadata-panel";
import { RawPanel } from "./raw-panel";
import { Segmented } from "./segmented";
import { SummaryPanel } from "./summary-panel";
import { SpansPanel } from "./spans-panel";
import { TicksPanel } from "./ticks-panel";
import { WorldPanel } from "./world-panel";
import { NetworkPanel } from "./network-panel";
import { MemoryPanel } from "./memory-panel";
import { BenchPanel } from "./bench-panel";
import { MarksPanel } from "./marks-panel";
import { WarningsPanel } from "./warnings-panel";
import { HotPanel } from "./hot-panel";
import { TreePanel } from "./tree-panel";
import type { CaptureRef, LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

const PANELS: Record<Exclude<Tab, "compare">, ComponentType<{ reader: HorizonPerf; capture: LoadedCapture }>> = {
  summary: SummaryPanel,
  metadata: MetadataPanel,
  gpu: GpuPanel,
  spans: SpansPanel,
  ticks: TicksPanel,
  world: WorldPanel,
  net: NetworkPanel,
  memory: MemoryPanel,
  bench: BenchPanel,
  marks: MarksPanel,
  warnings: WarningsPanel,
  hot: HotPanel,
  tree: TreePanel,
  query: RawPanel,
};

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
  showSide: (side: Side) => void;
  onClose: () => void;
}

/** Everything shown for an open capture. */
export function CaptureView({ reader, capture, showSide, onClose }: Props) {
  const [picked, setPicked] = useState<Tab>("summary");
  const [compareCapture, setCompareCapture] = useState<CaptureRef | null>(null);
  const compareHandle = useRef<number | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const sides = capture.sides.filter((side) => side !== "unspecified");
  const tabs = useMemo(() => {
    const ask = <R,>(op: string) => {
      try {
        const reply = reader.query<R>(capture.handle, { op, file: capture.name, filters: { side: capture.side } });
        return reply.ok ? reply.rows : [];
      } catch {
        return [];
      }
    };
    const hasTraffic = ask<NetTotalRow | NetPlayerRow>("net").some(
      (row) => "players" in row && row.out_kib_s + row.in_kib_s > 0,
    );
    return tabsFor((query) => ask(query).length > 0, hasTraffic);
  }, [reader, capture.handle, capture.name, capture.side]);
  const tab = tabs.includes(picked) ? picked : "summary";

  const chooseCompare = async (file: File) => {
    try {
      const handle = reader.open(await file.arrayBuffer());
      if (compareHandle.current !== null) reader.close(compareHandle.current);
      compareHandle.current = handle;
      setCompareCapture({ handle, name: file.name });
    } catch (failure) {
      setActionError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  useEffect(
    () => () => {
      if (compareHandle.current !== null) reader.close(compareHandle.current);
      compareHandle.current = null;
    },
    [reader],
  );

  const exportCapture = (kind: string) => {
    try {
      const bytes = reader.export(capture.handle, { kind });
      const url = URL.createObjectURL(new Blob([bytes as unknown as BlobPart], { type: "application/octet-stream" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${capture.name}.${kind === "perfetto" ? "pb" : kind === "folded" ? "folded" : "pprof"}`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (failure) {
      setActionError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const tabLabel = (id: Tab) => {
    if (id !== "ticks") return t(`tab.${id}`);
    return t(capture.side === "client" ? "tab.slowFrames" : "tab.slowTicks");
  };

  return (
    <>
      <header className={styles.captureBar}>
        <span className={styles.brand}>{t("app.brand")}</span>
        <div className={styles.actions}>
          {sides.length > 1 ? (
            <Segmented
              label={t("app.side")}
              options={sides.map((side) => ({ id: side, text: t(`side.${side}`) }))}
              value={capture.side}
              onChange={showSide}
            />
          ) : null}
          <select
            className={styles.select}
            aria-label={t("tab.export")}
            value=""
            onChange={(event) => {
              if (event.target.value) exportCapture(event.target.value);
            }}
          >
            <option value="">{t("tab.export")}</option>
            <option value="pprof">pprof</option>
            <option value="folded">folded</option>
            <option value="perfetto">Perfetto</option>
          </select>
          <button type="button" className={styles.buttonSmall} onClick={onClose}>
            {t("app.close")}
          </button>
        </div>
      </header>
      {actionError ? <p className={styles.error}>{actionError}</p> : null}

      <nav className={styles.tabBar} aria-label={t("app.tabs")}>
        {tabs.map((id) => (
          <button
            key={id}
            type="button"
            className={tab === id ? styles.tabActive : styles.tab}
            onClick={() => setPicked(id)}
          >
            {tabLabel(id)}
          </button>
        ))}
      </nav>

      <section key={tab} className={styles.panel}>
        {tab === "compare" ? (
          <ComparePanel reader={reader} base={capture} side={capture.side} other={compareCapture} onPick={(file) => void chooseCompare(file)} />
        ) : (
          createElement(PANELS[tab], { reader, capture })
        )}
      </section>
    </>
  );
}
