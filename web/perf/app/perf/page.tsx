"use client";

import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Icons } from "./icons";
import type { DragEvent, KeyboardEvent } from "react";
import { grouped } from "@/lib/format";
import { t } from "@/lib/i18n";
import { HealthPanel } from "./health-panel";
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
import { useReader } from "./use-capture";
import styles from "./page.module.css";

type Tab =
  | "summary"
  | "health"
  | "metadata"
  | "spans"
  | "ticks"
  | "world"
  | "net"
  | "memory"
  | "marks"
  | "warnings"
  | "hot"
  | "tree"
  | "query";

interface TabSpec {
  id: Tab;
  icon: ReactNode;
  label: string;
  initialRequest?: string;
}

const PRESET = (op: string, extra = ""): string =>
  `{
  "op": "${op}"${extra ? `,\n  ${extra}` : ""}
}`;

const TABS: TabSpec[] = [
  { id: "summary", icon: <Icons.summary />, label: "tab.summary" },
  { id: "health", icon: <Icons.health />, label: "tab.health" },
  { id: "metadata", icon: <Icons.metadata />, label: "tab.metadata" },
  { id: "spans", icon: <Icons.spans />, label: "tab.spans", initialRequest: PRESET("spans", '"filters": { "limit": 30 }') },
  { id: "ticks", icon: <Icons.ticks />, label: "tab.ticks", initialRequest: PRESET("ticks", '"filters": { "limit": 25 }') },
  { id: "world", icon: <Icons.world />, label: "tab.world", initialRequest: PRESET("world", '"filters": { "limit": 40 }') },
  { id: "net", icon: <Icons.net />, label: "tab.net", initialRequest: PRESET("net") },
  { id: "memory", icon: <Icons.memory />, label: "tab.memory", initialRequest: PRESET("memory") },
  { id: "marks", icon: <Icons.marks />, label: "tab.marks", initialRequest: PRESET("marks") },
  { id: "warnings", icon: <Icons.warnings />, label: "tab.warnings", initialRequest: PRESET("warnings") },
  { id: "hot", icon: <Icons.hot />, label: "tab.hot", initialRequest: PRESET("hot", '"filters": { "limit": 40 }') },
  { id: "tree", icon: <Icons.tree />, label: "tab.tree", initialRequest: PRESET("tree", '"filters": { "limit": 80 }') },
  { id: "query", icon: <Icons.query />, label: "tab.query" },
];

export default function PerfPage() {
  const { reader, readerError, capture, loading, error, open, close, showSide } = useReader();
  const [tab, setTab] = useState<Tab>("summary");
  const [dragging, setDragging] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const choose = async (files: FileList | File[] | null) => {
    const file = Array.isArray(files) ? files[0] : files?.[0];
    if (file) await open(file, file.name);
  };

  const drop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDragging(false);
    void choose(event.dataTransfer.files);
  };

  const openPicker = () => input.current?.click();

  const onDropzoneKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openPicker();
    }
  };

  useEffect(() => {
    const handler = (event: ClipboardEvent) => {
      const files = event.clipboardData?.files;
      if (files && files.length > 0) {
        event.preventDefault();
        void choose(files);
      }
    };
    window.addEventListener("paste", handler);
    return () => window.removeEventListener("paste", handler);
  }, [reader]);

  return (
    <main
      className={styles.page}
      onDragOver={(event) => {
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={drop}
    >
      <header className={styles.header}>
        <div>
          <h1 className={styles.title}>{t("app.title")}</h1>
          <p className={styles.muted}>{t("app.subtitle")}</p>
        </div>
        <div className={styles.actions}>
          <input
            ref={input}
            type="file"
            accept=".hzperf,application/octet-stream"
            className={styles.fileInput}
            onChange={(event) => void choose(event.target.files)}
          />
          <button type="button" className={styles.button} onClick={openPicker}>
            {t("app.choose")}
          </button>
          <span className={styles.muted}>{t("app.pasteHint")}</span>
        </div>
      </header>

      {readerError ? (
        <p className={styles.error}>{t("error.reader", { message: readerError })}</p>
      ) : null}
      {!reader && !readerError ? <p className={styles.muted}>{t("app.loadingReader")}</p> : null}
      {error ? <p className={styles.error}>{t("error.capture", { message: error })}</p> : null}

      {capture ? (
        <>
          <section className={styles.captureBar}>
            <div className={styles.captureName}>
              <strong>{capture.name}</strong>
              <span className={styles.muted}>{grouped(capture.bytes / 1024)} KiB</span>
            </div>
            <div className={styles.actions}>
              {capture.sides
                .filter((side) => side !== "unspecified")
                .map((side) => (
                  <button
                    key={side}
                    type="button"
                    className={side === capture.side ? styles.tabActive : styles.tab}
                    onClick={() => showSide(side)}
                  >
                    {t(`side.${side}`)}
                  </button>
                ))}
              <button type="button" className={styles.tab} onClick={close}>
                {t("app.close")}
              </button>
            </div>
          </section>

          <nav className={styles.tabBar} aria-label={t("app.tabs")}>
            {TABS.map(({ id, icon, label }) => (
              <button
                key={id}
                type="button"
                className={tab === id ? styles.tabActive : styles.tab}
                onClick={() => setTab(id)}
              >
                <span aria-hidden="true">{icon}</span> {t(label)}
              </button>
            ))}
          </nav>

          <section className={styles.panel}>
            {reader ? (
              tab === "summary" ? (
                <SummaryPanel reader={reader} capture={capture} />
              ) : tab === "health" ? (
                <HealthPanel reader={reader} capture={capture} />
              ) : tab === "metadata" ? (
                <MetadataPanel reader={reader} capture={capture} />
              ) : tab === "spans" ? (
                <SpansPanel reader={reader} capture={capture} />
              ) : tab === "ticks" ? (
                <TicksPanel reader={reader} capture={capture} />
              ) : tab === "world" ? (
                <WorldPanel reader={reader} capture={capture} />
              ) : tab === "net" ? (
                <NetworkPanel reader={reader} capture={capture} />
              ) : tab === "memory" ? (
                <MemoryPanel reader={reader} capture={capture} />
              ) : tab === "marks" ? (
                <MarksPanel reader={reader} capture={capture} />
              ) : tab === "warnings" ? (
                <WarningsPanel reader={reader} capture={capture} />
              ) : tab === "hot" ? (
                <HotPanel reader={reader} capture={capture} />
              ) : tab === "tree" ? (
                <TreePanel reader={reader} capture={capture} />
              ) : (
                <RawPanel
                  key={tab}
                  reader={reader}
                  capture={capture}
                  initialRequest={TABS.find((spec) => spec.id === tab)?.initialRequest}
                />
              )
            ) : null}
          </section>
        </>
      ) : (
        <div
          role="button"
          tabIndex={0}
          className={dragging ? styles.dropActive : styles.drop}
          onClick={openPicker}
          onKeyDown={onDropzoneKey}
        >
          <p className={styles.dropTitle}>
            {loading ? t("app.reading") : t("app.drop")}
          </p>
          <p className={styles.muted}>{t("app.privacy")}</p>
          <p className={styles.muted}>{t("app.dropHint")}</p>
        </div>
      )}
    </main>
  );
}