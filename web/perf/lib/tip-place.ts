// Where a tooltip goes: centred on its anchor, above it when there is room, kept inside the window.

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export type TipSide = "above" | "below";

export interface TipPlace {
  left: number;
  top: number;
  side: TipSide;
}

export const TIP_GAP = 6;
export const TIP_MARGIN = 8;

export function placeTip(anchor: Box, tip: Size, view: Size, gap = TIP_GAP, margin = TIP_MARGIN): TipPlace {
  const above = anchor.top - gap - tip.height;
  const side: TipSide = above >= margin ? "above" : "below";
  const top = side === "above" ? above : anchor.top + anchor.height + gap;
  const centred = anchor.left + anchor.width / 2 - tip.width / 2;
  const room = Math.max(margin, view.width - tip.width - margin);
  return { left: Math.min(Math.max(centred, margin), room), top, side };
}
