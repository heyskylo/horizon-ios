"use client";

import { useEffect, useState } from "react";
import { useParams } from "next/navigation";
import { useReader } from "../../use-capture";
import styles from "../../page.module.css";

export default function SharedCapturePage() {
  const params = useParams<{ id: string }>();
  const id = params?.id;
  const { reader, readerError, open, loading, error } = useReader();
  const [status, setStatus] = useState<string>("loading");

  useEffect(() => {
    if (!reader || !id) return;
    let live = true;
    void (async () => {
      try {
        const response = await fetch(`/api/perf/captures/${id}`);
        if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
        const bytes = await response.arrayBuffer();
        await open(bytes, id);
        if (live) setStatus("ready");
      } catch (failure) {
        if (live) setStatus(failure instanceof Error ? failure.message : String(failure));
      }
    })();
    return () => {
      live = false;
    };
  }, [reader, id, open]);

  if (readerError) return <p className={styles.error}>{readerError}</p>;
  if (loading || status === "loading") return <p className={styles.muted}>Loading…</p>;
  if (error || status !== "ready") {
    return (
      <main className={styles.page}>
        <p className={styles.error}>
          Could not open {id}: {error ?? status}. The upload API (U1) must be deployed for shared links.
        </p>
      </main>
    );
  }
  return (
    <main className={styles.page}>
      <p className={styles.muted}>Opened {id}.</p>
    </main>
  );
}
