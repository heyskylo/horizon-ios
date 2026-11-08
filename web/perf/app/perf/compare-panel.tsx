"use client";

import { useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { t } from "@/lib/i18n";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  base: LoadedCapture;
  other: LoadedCapture | null;
  onPick: (file: File) => void;
}

export function ComparePanel({ reader, base, other, onPick }: Props) {
  const [result, setResult] = useState<{ text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const input = (globalThis as { HTMLInputElement?: unknown }).HTMLInputElement;

  const run = () => {
    if (!other) return;
    setError(null);
    try {
      const reply = reader.diff(base.handle, other.handle, {});
      setResult({ text: reply.text });
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const verdictClass = (line: string) => {
    if (/faster/i.test(line)) return "#5bdb8a";
    if (/slower/i.test(line)) return "#ff6b6b";
    return "#e6e9ef";
  };

  return (
    <div className={styles.metadata}>
      <div className={styles.actions}>
        <input
          type="file"
          accept=".hzperf"
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) onPick(file);
          }}
        />
        <button type="button" className={styles.button} onClick={run} disabled={!other}>
          Compare
        </button>
      </div>
      {error ? <p className={styles.error}>{error}</p> : null}
      {other ? (
        <p className={styles.muted}>
          {base.name} vs {other.name}
        </p>
      ) : (
        <p className={styles.muted}>Pick a second capture to compare.</p>
      )}
      {result ? (
        <pre className={styles.pre}>
          {result.text.split("\n").map((line, index) => (
            <span key={index} style={{ color: verdictClass(line) }}>
              {line}
              {"\n"}
            </span>
          ))}
        </pre>
      ) : null}
    </div>
  );
}
