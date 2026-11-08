"use client";

import { useEffect, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { t } from "@/lib/i18n";
import type { QueryRequest } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

const DEFAULT_REQUEST = `{
  "op": "hot",
  "filters": { "limit": 25 }
}`;

/**
 * The raw bridge of the W1 skeleton: a query request as JSON, the wasm's reply as JSON. Every
 * panel's numbers can be checked here against `horizon-perf query --json`.
 */
export function RawPanel({ reader, capture }: Props) {
  const [request, setRequest] = useState(DEFAULT_REQUEST);
  const [reply, setReply] = useState("");
  const [error, setError] = useState<string | null>(null);

  const run = () => {
    setError(null);
    try {
      const parsed = JSON.parse(request) as QueryRequest;
      parsed.file = capture.name;
      parsed.filters = { side: capture.side, ...parsed.filters };
      setReply(JSON.stringify(reader.query(capture.handle, parsed), null, 2));
    } catch (failure) {
      setReply("");
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };

  // Run the preset (and again when the capture's side or handle changes) so each tab is not empty.
  useEffect(() => {
    run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reader, capture.handle, capture.side]);

  return (
    <div className={styles.raw}>
      <label className={styles.field}>
        <span>{t("raw.request")}</span>
        <textarea
          className={styles.textarea}
          value={request}
          spellCheck={false}
          onChange={(event) => setRequest(event.target.value)}
        />
      </label>
      <button type="button" className={styles.button} onClick={run}>
        {t("raw.run")}
      </button>
      {error ? <p className={styles.error}>{error}</p> : null}
      <label className={styles.field}>
        <span>{t("raw.reply")}</span>
        <pre className={styles.pre} aria-label={t("raw.reply")}>
          {reply}
        </pre>
      </label>
    </div>
  );
}
