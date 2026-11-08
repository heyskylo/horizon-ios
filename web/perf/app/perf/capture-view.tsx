"use client";

import { createElement, useEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { grouped } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { Side } from "@/lib/types";
import { HealthPanel } from "./health-panel";
import { GpuPanel } from "./gpu-panel";
import { ComparePanel } from "./compare-panel";
import { MetadataPanel } from "./metadata-panel";
import { RawPanel } from "./raw-panel";
import { SummaryPanel } from "./summary-panel";
import { SpansPanel } from "./spans-panel";
import { TicksPanel } from "./ticks-panel";
import { WorldPanel } from "./world-panel";
import { NetworkPanel } from "./network-panel";
import { MemoryPanel } from "./memory-panel";
import { MarksPanel } from "./marks-panel";
import { WarningsPanel } from "./warnings-panel";
import { HotPanel } from "./hot-panel";
import { TreePanel } from "./tree-panel";
import type { CaptureRef, LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

const TABS = [
  "summary",
  "health",
  "metadata",
  "gpu",
  "spans",
  "ticks",
  "world",
  "net",
  "memory",
  "marks",
  "warnings",
  "hot",
  "tree",
  "compare",
  "query",
] as const;

type Tab = (typeof TABS)[number];

const PANELS: Record<Exclude<Tab, "compare">, ComponentType<{ reader: HorizonPerf; capture: LoadedCapture }>> = {
  summary: SummaryPanel,
  health: HealthPanel,
  metadata: MetadataPanel,
  gpu: GpuPanel,
  spans: SpansPanel,
  ticks: TicksPanel,
  world: WorldPanel,
  net: NetworkPanel,
  memory: MemoryPanel,
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
  onNewProfile: () => void;
}

/** Everything shown for an open capture. */
export function CaptureView({ reader, capture, showSide, onClose, onNewProfile }: Props) {
  const [tab, setTab] = useState<Tab>("summary");
  const [compareCapture, setCompareCapture] = useState<CaptureRef | null>(null);
  const compareHandle = useRef<number | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const sides = capture.sides.filter((side) => side !== "unspecified");

  const chooseCompare = async (files: File[]) => {
    const file = files[0];
    if (!file) return;
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

  const exportCapture = async (kind: string) => {
        try {
      const bytes = reader.export(capture.handle, { kind });
      const blob = new Blob([bytes as unknown as BlobPart], { type: "application/octet-stream" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `${capture.name}.${kind === "perfetto" ? "pb" : kind === "folded" ? "folded" : "pprof"}`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (failure) {
      setActionError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  return (
    <>
    <header className={styles.captureBar}>
        <div className={styles.captureName}>
          <span className={styles.brand}>{t("app.brand")}</span>
          <span className={styles.micro}>{grouped(capture.bytes / 1024)} KiB</span>
        </div>
        <div className={styles.actions}>
          {sides.length > 1 ? (
            <div className={styles.segmented} role="group" aria-label={t("app.side")}>
              {sides.map((side) => (
                <button
                  key={side}
                  type="button"
                  className={side === capture.side ? styles.segmentActive : styles.segment}
                  aria-pressed={side === capture.side}
                  onClick={() => showSide(side)}
                >
                  {t(`side.${side}`)}
                </button>
              ))}
            </div>
          ) : null}
          <select
            className={styles.select}
            aria-label={t("tab.export")}
            value=""
            onChange={(event) => {
              if (event.target.value) void exportCapture(event.target.value);
            }}
          >
            <option value="">{t("tab.export")}</option>
            <option value="pprof">pprof</option>
            <option value="folded">folded</option>
            <option value="perfetto">Perfetto</option>
          </select>
          <button type="button" className={styles.buttonSmall} onClick={onNewProfile}>
            {t("app.newProfile")}
          </button>
          <button type="button" className={styles.buttonSmall} onClick={onClose}>
            {t("app.close")}
          </button>
        </div>
      </header>
      {actionError ? <p className={styles.error}>{actionError}</p> : null}

    <nav className={styles.tabBar} aria-label={t("app.tabs")}>
      {TABS.map((id) => (
          <button
            key={id}
            type="button"
            className={tab === id ? styles.tabActive : styles.tab}
            onClick={() => setTab(id)}
          >
            {t(`tab.${id}`)}
          </button>
        ))}
      </nav>

    <section key={tab} className={styles.panel}>
        {tab === "compare" ? (
          <ComparePanel reader={reader} base={capture} other={compareCapture} onPick={(file) => void chooseCompare([file])} />
        ) : (
          createElement(PANELS[tab], { reader, capture })
        )}
      </section>
    </>
  );
}
