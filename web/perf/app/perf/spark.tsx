"use client";

import { sparkline } from "@/lib/health";
import styles from "./page.module.css";

/** The drawn shape of a series (docs/perf-platform.md §7.2): the wasm's points, as a line. */
export function Spark({
  values,
  label,
  width = 220,
  height = 36,
}: {
  values: (number | null)[];
  label: string;
  width?: number;
  height?: number;
}) {
  const shape = sparkline(values, width, height);
  if (!shape) return null;
  return (
    <svg
      className={styles.sparkline}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={label}
    >
      <path d={shape.path} />
    </svg>
  );
}
