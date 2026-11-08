"use client";

import { useRef, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { t } from "@/lib/i18n";
import type { CaptureRef } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  base: CaptureRef;
  other: CaptureRef | null;
  onPick: (file: File) => void;
}

export function ComparePanel({ reader, base, other, onPick }: Props) {
  const [result, setResult] = useState<{ text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  const run = () => {
    if (!other) return;
    setError(null);
    setResult(null);
    try {
      const reply = reader.diff(base.handle, other.handle, {});
      if (reply.ok === false || typeof reply.text !== "string") {
        setError(reply.error?.message ?? t("error.queryFailed"));
        return;
      }
      setResult({ text: reply.text });
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  const verdictColor = (line: string) => {
    if (/faster/i.test(line)) return "var(--success)";
    if (/slower/i.test(line)) return "var(--danger)";
    return undefined;
  };

  return (
    <div className={styles.metadata}>
      <div className={styles.actions}>
        <input
          ref={picker}
          type="file"
          accept=".hzperf"
          className={styles.fileInput}
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) onPick(file);
          }}
        />
        <button type="button" className={styles.buttonSmall} onClick={() => picker.current?.click()}>
          {t("compare.choose")}
        </button>
        <button type="button" className={styles.button} onClick={run} disabled={!other}>
          {t("compare.run")}
        </button>
      </div>
      {error ? <p className={styles.error}>{error}</p> : null}
      {other ? (
        <p className={styles.muted}>
          {t("compare.pair", { a: base.name, b: other.name })}
        </p>
      ) : (
        <p className={styles.muted}>{t("compare.pick")}</p>
      )}
      {result ? (
        <pre className={styles.pre}>
          {result.text.split("\n").map((line, index) => (
            <span key={index} style={{ color: verdictColor(line) }}>
              {line}
              {"\n"}
            </span>
          ))}
        </pre>
      ) : null}
    </div>
  );
}
