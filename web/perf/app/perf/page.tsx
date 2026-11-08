"use client";

import { useEffect, useRef, useState } from "react";
import type { DragEvent } from "react";
import { t } from "@/lib/i18n";
import { CaptureView } from "./capture-view";
import { Landing } from "./landing";
import { useReader } from "./use-capture";
import styles from "./page.module.css";

export default function PerfPage() {
  const { reader, readerError, capture, error, open, close, showSide } = useReader();
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
      data-tip-root=""
      onDragOver={(event) => {
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={drop}
    >
      <input
        ref={input}
        type="file"
        accept=".hzperf,application/octet-stream"
        className={styles.fileInput}
        onChange={(event) => {
          void choose(event.target.files);
          event.target.value = "";
        }}
      />
      {capture && reader ? (
        <>
          {error ? <p className={styles.error}>{t("error.capture", { message: error })}</p> : null}
          <CaptureView reader={reader} capture={capture} showSide={showSide} onClose={close} />
        </>
      ) : (
        <Landing
          ready={Boolean(reader)}
          dragging={dragging}
          problem={
            readerError ? t("error.reader", { message: readerError }) : error ? t("error.capture", { message: error }) : null
          }
          onChoose={openPicker}
        />
      )}
    </main>
  );
}
