/** The trailing TPS windows the summary prints, in seconds, with the translation key of each label. */
export const TRAILING_WINDOWS = [
  { seconds: 60, label: "health.window1m" },
  { seconds: 300, label: "health.window5m" },
  { seconds: 900, label: "health.window15m" },
] as const;

export type TrailingWindow = (typeof TRAILING_WINDOWS)[number];

/** The windows a capture of `seconds` can fill: a 15-minute figure of a 3-second capture would only repeat its average. */
export function windowsWithin(seconds: number): TrailingWindow[] {
  return TRAILING_WINDOWS.filter((window) => window.seconds <= seconds);
}
