"use client";

import { useEffect, useRef } from "react";
import type { HotRow } from "@/lib/types";

interface Props {
  rows: HotRow[];
  width?: number;
  height?: number;
}

/** Canvas flame graph for the current selection (top rows by total). */
export function FlameGraph({ rows, width = 600, height = 180 }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, width, height);
    const sorted = [...rows].sort((a, b) => b.total - a.total).slice(0, 40);
    const max = sorted.reduce((m, r) => Math.max(m, r.total), 0) || 1;
    let y = 0;
    const rowHeight = height / Math.max(1, sorted.length);
    for (const row of sorted) {
      const w = (row.total / max) * width;
      ctx.fillStyle = "rgba(120, 160, 220, 0.7)";
      ctx.fillRect(0, y, w, rowHeight - 1);
      ctx.fillStyle = "#222";
      ctx.font = "10px sans-serif";
      ctx.fillText(row.name, 2, y + rowHeight - 3);
      y += rowHeight;
    }
  }, [rows, width, height]);

  return <canvas ref={ref} width={width} height={height} style={{ border: "1px solid #ccc" }} />;
}
