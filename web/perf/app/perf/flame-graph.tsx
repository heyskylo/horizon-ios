"use client";

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, RefObject } from "react";
import { crateGroup } from "@/lib/crate-group";
import type { CrateGroup } from "@/lib/crate-group";
import { fitText, frameAt, layoutFlame, moveFrom, nameMatches, zoomPath } from "@/lib/flame-model";
import type { FlameFrame, FlameMove } from "@/lib/flame-model";
import { num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { TreeRow } from "@/lib/types";
import { amount } from "@/lib/units";
import type { AmountKind } from "@/lib/units";
import { Swatch } from "./tree-rows";
import styles from "./page.module.css";
import tree from "./tree.module.css";

const ROW = 20;
const PAD = 6;
/** A frame under this many pixels wide is not drawn, nor what is below it. */
const SLIVER = 1.5;
/** Room a label needs before it is worth writing. */
const LABEL_MIN = 28;
const DIMMED = 0.28;

type Source = "pointer" | "keyboard";

interface Active {
  key: string;
  source: Source;
  /** Where the pointer was, in pixels across the graph. */
  x: number;
}

const KEY_MOVES: Record<string, FlameMove> = {
  ArrowUp: "parent",
  ArrowDown: "child",
  ArrowLeft: "previous",
  ArrowRight: "next",
};

/** The pixels of the page: they change with the browser's zoom and when the window moves between screens. */
function usePixelRatio(): number {
  const [ratio, setRatio] = useState(1);
  useEffect(() => {
    let query: MediaQueryList | null = null;
    const read = () => {
      query?.removeEventListener("change", read);
      setRatio(window.devicePixelRatio || 1);
      query = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      query.addEventListener("change", read);
    };
    read();
    return () => query?.removeEventListener("change", read);
  }, []);
  return ratio;
}

function useWidth(target: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = target.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.floor(entry.contentRect.width)));
    observer.observe(element);
    setWidth(Math.floor(element.getBoundingClientRect().width));
    return () => observer.disconnect();
  }, [target]);
  return width;
}

interface Palette {
  groups: Record<CrateGroup | "unknown", string>;
  ink: string;
  inkSoft: string;
  page: string;
  font: string;
}

function readPalette(canvas: HTMLCanvasElement): Palette {
  const style = getComputedStyle(canvas);
  const read = (name: string) => style.getPropertyValue(name).trim();
  return {
    groups: {
      world: read("--crate-world"),
      client: read("--crate-client"),
      horizon: read("--crate-horizon"),
      std: read("--crate-std"),
      third: read("--crate-third"),
      mod: read("--crate-mod"),
      unknown: read("--plate"),
    },
    ink: read("--ink"),
    inkSoft: read("--ink-soft"),
    page: read("--page"),
    font: `12px ${style.fontFamily}`,
  };
}

interface DrawArgs {
  canvas: HTMLCanvasElement;
  levels: readonly (readonly FlameFrame[])[];
  width: number;
  ratio: number;
  mods: ReadonlySet<string>;
  query: string;
  active: string | null;
}

function draw({ canvas, levels, width, ratio, mods, query, active }: DrawArgs) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const palette = readPalette(canvas);
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  ctx.clearRect(0, 0, width, canvas.height / ratio);
  ctx.font = palette.font;
  ctx.textBaseline = "middle";
  const measure = (text: string) => ctx.measureText(text).width;
  const searching = query.trim() !== "";
  const snap = (value: number) => Math.round(value * ratio) / ratio;
  levels.forEach((level, depth) => {
    for (const frame of level) {
      const left = snap(frame.start * width);
      const right = snap((frame.start + frame.width) * width);
      const wide = right - left - 1 / ratio;
      if (wide <= 0) continue;
      const top = depth * ROW;
      const group = crateGroup(frame.row.crate, mods) ?? "unknown";
      ctx.globalAlpha = searching && !nameMatches(frame.row.name, query) ? DIMMED : 1;
      ctx.fillStyle = palette.groups[group];
      ctx.fillRect(left, top, wide, ROW - 1);
      if (wide >= LABEL_MIN) {
        const label = fitText(frame.row.name, wide - PAD * 2, measure);
        ctx.fillStyle = group === "unknown" ? palette.inkSoft : palette.page;
        ctx.fillText(label, left + PAD, top + ROW / 2);
      }
      if (frame.key === active) {
        ctx.globalAlpha = 1;
        ctx.strokeStyle = palette.ink;
        ctx.lineWidth = 2;
        ctx.strokeRect(left + 1, top + 1, Math.max(wide - 2, 0), ROW - 3);
      }
    }
  });
  ctx.globalAlpha = 1;
}

interface Props {
  rows: readonly TreeRow[];
  mods: ReadonlySet<string>;
  kind: AmountKind;
  query: string;
}

/** The tree as a flame graph: time across, call depth down, the graph's top at the top. */
export function FlameGraph({ rows, mods, kind, query }: Props) {
  const [zoom, setZoom] = useState<string | null>(null);
  const [active, setActive] = useState<Active | null>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const touched = useRef(false);
  const width = useWidth(wrap);
  const ratio = usePixelRatio();
  const layout = useMemo(() => layoutFlame(rows, zoom, width > 0 ? SLIVER / width : 0), [rows, zoom, width]);
  const path = useMemo(() => zoomPath(rows, zoom), [rows, zoom]);
  const height = Math.max(layout.levels.length, 1) * ROW;
  const activeKey = active && layout.byKey.has(active.key) ? active.key : null;

  const paint = useCallback(() => {
    const element = canvas.current;
    if (element && width > 0) draw({ canvas: element, levels: layout.levels, width, ratio, mods, query, active: activeKey });
  }, [layout, width, ratio, mods, query, activeKey]);

  useLayoutEffect(() => {
    const element = canvas.current;
    if (!element || width === 0) return;
    element.width = Math.round(width * ratio);
    element.height = Math.round(height * ratio);
    paint();
  }, [width, height, ratio, paint]);

  useEffect(() => {
    void document.fonts?.ready.then(paint);
  }, [paint]);

  const frameUnder = (event: MouseEvent<HTMLCanvasElement>): { frame: FlameFrame | null; x: number } => {
    const box = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - box.left;
    return { frame: frameAt(layout, Math.floor((event.clientY - box.top) / ROW), x / box.width), x };
  };

  const zoomTo = (key: string | null) => {
    setZoom(key);
    setActive((current) => (current?.source === "keyboard" ? current : null));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLCanvasElement>) => {
    const move = KEY_MOVES[event.key];
    const from = activeKey ?? layout.levels[0]?.[0]?.key ?? null;
    if (move && from) {
      event.preventDefault();
      const next = activeKey ? moveFrom(layout, from, move) : from;
      if (next) setActive({ key: next, source: "keyboard", x: 0 });
    } else if ((event.key === "Enter" || event.key === " ") && activeKey) {
      event.preventDefault();
      zoomTo(activeKey);
    } else if ((event.key === "Escape" || event.key === "Backspace") && zoom !== null) {
      event.preventDefault();
      zoomTo(path.length > 1 ? path[path.length - 2].key : null);
    }
  };

  const shown = activeKey ? layout.byKey.get(activeKey) : undefined;
  const tipX = shown && active ? (active.source === "pointer" ? active.x : (shown.start + shown.width / 2) * width) : 0;
  const tipStyle: CSSProperties | undefined = shown
    ? {
        top: (shown.depth + 1) * ROW + 6,
        ...(tipX > width / 2 ? { right: Math.max(width - tipX, 0) } : { left: tipX }),
      }
    : undefined;
  const shownGroup = shown ? crateGroup(shown.row.crate, mods) : null;

  if (layout.levels.length === 0) return <p className={tree.none}>{t("tree.flame.empty")}</p>;
  return (
    <div className={tree.flame}>
      {zoom === null ? (
        <p className={`${styles.muted} ${tree.flameHint}`}>{t("tree.flame.hint")}</p>
      ) : (
        <nav className={tree.flameTrail} aria-label={t("tree.flame.trail")}>
          <button type="button" className={styles.buttonSmall} onClick={() => zoomTo(null)}>
            {t("tree.flame.whole")}
          </button>
          {path.map((step, at) =>
            at === path.length - 1 ? (
              <span key={step.key} className={tree.flameHere} aria-current="location">
                {step.row.name}
              </span>
            ) : (
              <button key={step.key} type="button" className={tree.flameCrumb} onClick={() => zoomTo(step.key)}>
                {step.row.name}
              </button>
            ),
          )}
        </nav>
      )}
      <div ref={wrap} className={tree.flameWrap}>
        <canvas
          ref={canvas}
          className={tree.flameCanvas}
          style={{ height, width }}
          tabIndex={0}
          role="img"
          aria-label={t("tree.flame.label")}
          onPointerDown={(event) => {
            touched.current = event.pointerType === "touch";
          }}
          onPointerMove={(event) => {
            const { frame, x } = frameUnder(event);
            setActive(frame ? { key: frame.key, source: "pointer", x } : null);
          }}
          onPointerLeave={() => {
            if (!touched.current) setActive((current) => (current?.source === "pointer" ? null : current));
          }}
          onClick={(event) => {
            const { frame, x } = frameUnder(event);
            if (!touched.current || frame?.key === activeKey) {
              if (frame) zoomTo(frame.key);
            } else {
              setActive(frame ? { key: frame.key, source: "pointer", x } : null);
            }
          }}
          onKeyDown={onKeyDown}
          onBlur={() => setActive((current) => (current?.source === "keyboard" ? null : current))}
        />
        {shown && tipStyle ? (
          <div className={tree.flameTip} style={tipStyle} role="status" aria-live={active?.source === "keyboard" ? "polite" : "off"}>
            <span className={tree.flameTipName}>{shown.row.name}</span>
            <span>
              {amount(shown.row.total, kind)} · {num(shown.row.pct, 1)}%
            </span>
            <span className={tree.flameTipCrate}>
              <Swatch group={shownGroup} />
              {t(`tree.crate.${shownGroup ?? "unknown"}`)}
            </span>
          </div>
        ) : null}
      </div>
    </div>
  );
}
